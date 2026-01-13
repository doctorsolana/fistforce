//! Client-side weapon systems
//!
//! Handles shooting input, local tracers for prediction, and weapon effects.
//! Updated for Lightyear 0.25 / Bevy 0.17

use bevy::prelude::*;
use bevy::audio::Volume;
use bevy::diagnostic::{DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin};
use lightyear::prelude::*;
use shared::{
    weapons::{ballistics, WeaponDebugMode, WeaponType},
    Bullet, BulletImpact, BulletImpactSurface, BulletVelocity, EquippedWeapon, HitConfirm, LocalTracer,
    ChunkCoord, LocalPlayer, Player, PlayerPosition, ShootRequest, ReloadRequest, ReliableChannel, WorldTerrain,
};

/// Marker for the debug overlay UI
#[derive(Component)]
pub struct DebugOverlay;

/// Marker for the FPS text specifically
#[derive(Component)]
pub struct FpsText;

/// Multi-line performance stats text in debug overlay
#[derive(Component)]
pub struct PerfStatsText;

/// Cached weapon-related render assets (avoid per-shot allocations).
#[derive(Resource)]
pub struct WeaponVisualAssets {
    pub tracer_mesh: Handle<Mesh>,
    pub tracer_material: Handle<StandardMaterial>,
    pub impact_disk_mesh_unit: Handle<Mesh>,
    pub blood_splatter_mesh: Handle<Mesh>,      // Flat disk for ground splats
    pub blood_droplet_mesh: Handle<Mesh>,       // Small sphere for flying droplets
    pub blood_burst_mesh: Handle<Mesh>,         // Larger sphere for instant burst
    pub blood_droplet_material: Handle<StandardMaterial>,
    pub blood_pool_material: Handle<StandardMaterial>,
    pub blood_burst_material: Handle<StandardMaterial>,
}

/// Audio assets for weapon sound effects
#[derive(Resource)]
pub struct WeaponAudioAssets {
    pub gun_shot: Handle<AudioSource>,
    pub shotgun_shot: Handle<AudioSource>,
    pub out_of_ammo: Handle<AudioSource>,
    pub gun_reload: Handle<AudioSource>,
}

/// Load weapon audio assets at startup
pub fn setup_weapon_audio_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    commands.insert_resource(WeaponAudioAssets {
        gun_shot: asset_server.load("audio/sfx/gun_shot.ogg"),
        shotgun_shot: asset_server.load("audio/sfx/shutgun_shot.ogg"),
        out_of_ammo: asset_server.load("audio/sfx/out_of_ammo.ogg"),
        gun_reload: asset_server.load("audio/sfx/gun_reload.ogg"),
    });
}

use crate::crosshair;
use crate::input::InputState;
use crate::states::GameState;

/// Resource to track shooting state
#[derive(Resource)]
pub struct ShootingState {
    pub fire_held: bool,
    pub last_fire_time: f32,
    /// Set to true when a shot was fired this frame (for audio)
    pub shot_fired_this_frame: bool,
    /// Set when player tries to shoot with no ammo
    pub out_of_ammo_this_frame: bool,
    /// Track weapon type that fired (for shotgun vs other sounds)
    pub weapon_fired: Option<WeaponType>,
    /// Accumulated vertical recoil (pitch) from rapid fire
    pub accumulated_recoil_pitch: f32,
    /// Accumulated horizontal recoil (yaw) from rapid fire
    pub accumulated_recoil_yaw: f32,
    /// Number of shots in current burst (resets after pause)
    pub shots_in_burst: u32,
    /// Last time out of ammo sound was played (to avoid spam)
    pub last_out_of_ammo_time: f32,
}

impl Default for ShootingState {
    fn default() -> Self {
        Self {
            fire_held: false,
            last_fire_time: -10.0,
            shot_fired_this_frame: false,
            out_of_ammo_this_frame: false,
            weapon_fired: None,
            accumulated_recoil_pitch: 0.0,
            accumulated_recoil_yaw: 0.0,
            shots_in_burst: 0,
            last_out_of_ammo_time: -10.0,
        }
    }
}

/// Resource to track reload state for audio
#[derive(Resource, Default)]
pub struct ReloadState {
    pub reload_requested_this_frame: bool,
}

// Recoil settings
const RECOIL_RECOVERY_SPEED: f32 = 4.0; // How fast recoil recovers (radians/sec)
const RECOIL_BURST_RESET_TIME: f32 = 0.35; // Time without shooting to reset burst counter
const RECOIL_ADS_MULTIPLIER: f32 = 0.5; // ADS reduces recoil by 50%
const RECOIL_ACCUMULATION_MULT: f32 = 1.15; // Each shot in burst adds 15% more recoil

/// Component for bullet impact markers
#[derive(Component)]
pub struct ImpactMarker {
    pub spawn_time: f32,
    pub lifetime: f32,
    pub color: Color,
    pub base_scale: f32,
}

/// Flying blood droplet with physics (gravity + velocity)
#[derive(Component)]
pub struct BloodDroplet {
    pub velocity: Vec3,
    pub spawn_time: f32,
}

/// Quick splash ring when a droplet hits the ground (expands fast then fades)
#[derive(Component)]
pub struct BloodSplashRing {
    pub spawn_time: f32,
    pub lifetime: f32,
    pub initial_scale: f32,
}

/// Blood splat on the ground (static decal that fades)
#[derive(Component)]
pub struct BloodGroundSplat {
    pub spawn_time: f32,
    pub lifetime: f32,
    pub initial_scale: f32,
}

/// Instant blood burst for hit feedback - expands quickly and fades fast (PUBG-style)
#[derive(Component)]
pub struct BloodBurst {
    pub spawn_time: f32,
    pub lifetime: f32,       // Short! ~0.25s
    pub initial_scale: f32,
    pub max_scale: f32,      // How big it grows
    pub direction: Vec3,     // Bias expansion direction (away from shooter)
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
    mut input_state: ResMut<InputState>,
    local_player: Query<&EquippedWeapon, With<LocalPlayer>>,
    camera: Query<&Transform, With<Camera3d>>,
    time: Res<Time>,
    game_state: Res<State<GameState>>,
    mut last_warn_time: Local<f32>,
) {
    // Reset flags each frame
    shooting_state.shot_fired_this_frame = false;
    shooting_state.out_of_ammo_this_frame = false;
    shooting_state.weapon_fired = None;
    
    // Don't shoot if paused
    if game_state.get() != &GameState::Playing {
        return;
    }
    
    // Don't shoot while dead
    if input_state.is_dead {
        return;
    }
    
    // Don't shoot while inventory is open
    if input_state.inventory_open {
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
    let cooldown_passed = (current_time - shooting_state.last_fire_time) >= cooldown;
    let has_ammo = weapon.ammo_in_mag > 0;
    
    // Out of ammo click (only on just pressed, not held, and with rate limiting)
    if fire_pressed && !has_ammo && cooldown_passed {
        if current_time - shooting_state.last_out_of_ammo_time > 0.3 {
            shooting_state.out_of_ammo_this_frame = true;
            shooting_state.last_out_of_ammo_time = current_time;
        }
    }
    
    let can_fire = has_ammo && cooldown_passed;
    
    if fire_pressed && can_fire {
        shooting_state.last_fire_time = current_time;
        
        // Get aim direction from camera
        let direction = camera_transform.forward().as_vec3();
        
        // Use the toggled ADS state from input (not mouse.pressed since we switched to toggle)
        let aiming = input_state.aiming;
        
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

        // === APPLY RECOIL ===
        let stats = weapon.weapon_type.stats();
        
        // Accumulation multiplier based on burst length (more shots = more recoil)
        let burst_mult = RECOIL_ACCUMULATION_MULT.powi(shooting_state.shots_in_burst as i32);
        
        // ADS reduces recoil
        let ads_mult = if aiming { RECOIL_ADS_MULTIPLIER } else { 1.0 };
        
        // Calculate recoil for this shot
        let vertical_recoil = stats.recoil_vertical * burst_mult * ads_mult;
        let horizontal_recoil = stats.recoil_horizontal * burst_mult * ads_mult;
        
        // Random horizontal direction (left or right)
        let h_direction = if rand::random::<bool>() { 1.0 } else { -1.0 };
        // Add some randomness to horizontal (not always max)
        let h_random = 0.3 + rand::random::<f32>() * 0.7;
        
        // Apply recoil to camera pitch (kick up) and yaw (kick sideways)
        input_state.pitch += vertical_recoil;
        input_state.yaw += horizontal_recoil * h_direction * h_random;
        
        // Clamp pitch to valid range
        input_state.pitch = input_state.pitch.clamp(
            -std::f32::consts::FRAC_PI_2 + 0.01, 
            std::f32::consts::FRAC_PI_2 - 0.01
        );
        
        // Track accumulated recoil (for recovery system)
        shooting_state.accumulated_recoil_pitch += vertical_recoil;
        shooting_state.accumulated_recoil_yaw += horizontal_recoil * h_direction * h_random;
        
        // Increment burst counter
        shooting_state.shots_in_burst += 1;

        // Mark that we fired for audio system
        shooting_state.shot_fired_this_frame = true;
        shooting_state.weapon_fired = Some(weapon.weapon_type);
    }
    
    shooting_state.fire_held = fire_pressed;
}

/// Recover recoil over time when not shooting
pub fn recover_recoil(
    mut shooting_state: ResMut<ShootingState>,
    mut input_state: ResMut<InputState>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let current_time = time.elapsed_secs();
    
    // Reset burst counter if haven't shot recently
    if current_time - shooting_state.last_fire_time > RECOIL_BURST_RESET_TIME {
        shooting_state.shots_in_burst = 0;
    }
    
    // Recover recoil gradually (pull aim back down)
    if shooting_state.accumulated_recoil_pitch.abs() > 0.001 {
        let recovery = RECOIL_RECOVERY_SPEED * dt;
        
        // Recover pitch (vertical)
        if shooting_state.accumulated_recoil_pitch > 0.0 {
            let recover_amount = recovery.min(shooting_state.accumulated_recoil_pitch);
            input_state.pitch -= recover_amount;
            shooting_state.accumulated_recoil_pitch -= recover_amount;
        } else {
            let recover_amount = recovery.min(-shooting_state.accumulated_recoil_pitch);
            input_state.pitch += recover_amount;
            shooting_state.accumulated_recoil_pitch += recover_amount;
        }
        
        // Clamp pitch
        input_state.pitch = input_state.pitch.clamp(
            -std::f32::consts::FRAC_PI_2 + 0.01, 
            std::f32::consts::FRAC_PI_2 - 0.01
        );
    }
    
    // Recover yaw (horizontal) - faster recovery
    if shooting_state.accumulated_recoil_yaw.abs() > 0.001 {
        let recovery = RECOIL_RECOVERY_SPEED * 1.5 * dt;
        
        if shooting_state.accumulated_recoil_yaw > 0.0 {
            let recover_amount = recovery.min(shooting_state.accumulated_recoil_yaw);
            input_state.yaw -= recover_amount;
            shooting_state.accumulated_recoil_yaw -= recover_amount;
        } else {
            let recover_amount = recovery.min(-shooting_state.accumulated_recoil_yaw);
            input_state.yaw += recover_amount;
            shooting_state.accumulated_recoil_yaw += recover_amount;
        }
    }
}

/// Handle reload input - sends request to server
pub fn handle_reload_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut client_query: Query<&mut MessageSender<ReloadRequest>, (With<crate::GameClient>, With<Connected>)>,
    local_player: Query<(&EquippedWeapon, &shared::Inventory), With<LocalPlayer>>,
    input_state: Res<InputState>,
    mut reload_state: ResMut<ReloadState>,
) {
    // Reset flag each frame
    reload_state.reload_requested_this_frame = false;
    
    // Don't reload while dead
    if input_state.is_dead {
        return;
    }
    
    // Don't reload in vehicle
    if input_state.in_vehicle {
        return;
    }
    
    if keyboard.just_pressed(KeyCode::KeyR) {
        if let Ok((weapon, inventory)) = local_player.single() {
            // Only send reload request if we actually need ammo and have reserve in inventory
            let stats = weapon.weapon_type.stats();
            let reserve_in_inventory = inventory.count_item(weapon.weapon_type.ammo_type());
            if weapon.ammo_in_mag < stats.magazine_size && reserve_in_inventory > 0 {
                if let Ok(mut sender) = client_query.single_mut() {
                    let _ = sender.send::<ReliableChannel>(ReloadRequest);
                }
                reload_state.reload_requested_this_frame = true;
                info!("Reload requested...");
            }
        }
    }
}

/// Play weapon sound effects based on shooting/reload state
pub fn play_weapon_sounds(
    mut commands: Commands,
    audio_assets: Option<Res<WeaponAudioAssets>>,
    shooting_state: Res<ShootingState>,
    reload_state: Res<ReloadState>,
) {
    let Some(audio) = audio_assets else { return };
    
    // Play shot sound
    if shooting_state.shot_fired_this_frame {
        let sound = if let Some(weapon_type) = shooting_state.weapon_fired {
            if weapon_type == WeaponType::Shotgun {
                audio.shotgun_shot.clone()
            } else {
                audio.gun_shot.clone()
            }
        } else {
            audio.gun_shot.clone()
        };
        
        commands.spawn((
            AudioPlayer::new(sound),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.5)),
        ));
    }
    
    // Play out of ammo click
    if shooting_state.out_of_ammo_this_frame {
        commands.spawn((
            AudioPlayer::new(audio.out_of_ammo.clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.6)),
        ));
    }
    
    // Play reload sound
    if reload_state.reload_requested_this_frame {
        commands.spawn((
            AudioPlayer::new(audio.gun_reload.clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.5)),
        ));
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
    weapon_visuals: Option<Res<WeaponVisualAssets>>,
    bullets: Query<(Entity, &Bullet, &BulletVelocity, Option<&PlayerPosition>), Added<Bullet>>,
) {
    let Some(weapon_visuals) = weapon_visuals else {
        return;
    };

    for (entity, bullet, velocity, position) in bullets.iter() {
        let spawn_pos = position.map(|p| p.0).unwrap_or(bullet.spawn_position);

        // Calculate initial rotation from velocity
        let direction = velocity.0.normalize_or_zero();
        let rotation = if direction.length() > 0.1 {
            Quat::from_rotation_arc(Vec3::Y, direction)
        } else {
            Quat::IDENTITY
        };
        
        commands.entity(entity).insert((
            Mesh3d(weapon_visuals.tracer_mesh.clone()),
            MeshMaterial3d(weapon_visuals.tracer_material.clone()),
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
    weapon_visuals: Option<Res<WeaponVisualAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    // In Lightyear 0.25, we receive messages via MessageReceiver component
    mut client_query: Query<&mut MessageReceiver<BulletImpact>, (With<crate::GameClient>, With<Connected>)>,
    time: Res<Time>,
    terrain: Option<Res<WorldTerrain>>,
    debug_mode: Res<WeaponDebugMode>,
    mut debug_trails: ResMut<DebugBulletTrails>,
) {
    let Some(weapon_visuals) = weapon_visuals else {
        return;
    };

    let now = time.elapsed_secs();

    // Keep trails bounded even if debug is toggled on/off
    debug_trails
        .trails
        .retain(|(_trail, spawn_time, _color)| now - *spawn_time <= 10.0);

    let Ok(mut receiver) = client_query.single_mut() else {
        return;
    };

    for impact in receiver.receive() {
        let normal = impact.impact_normal.normalize_or_zero();
        let offset = if normal.length_squared() > 0.001 { normal * 0.03 } else { Vec3::Y * 0.03 };

        match impact.surface {
            BulletImpactSurface::Terrain | BulletImpactSurface::PracticeWall => {
                // Choose marker visuals by surface
                let (base_color, radius) = match impact.surface {
                    BulletImpactSurface::Terrain => (Color::srgba(1.0, 0.5, 0.0, 0.85), 0.35),
                    BulletImpactSurface::PracticeWall => (Color::srgba(1.0, 0.0, 0.0, 0.9), 0.25),
                    _ => unreachable!(),
                };

                // Thin "bullet hole / scorch" disk (cylinder)
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
                        base_scale: radius,
                    },
                    Mesh3d(weapon_visuals.impact_disk_mesh_unit.clone()),
                    MeshMaterial3d(marker_material),
                    Transform::from_translation(impact.impact_position + offset)
                        .with_rotation(rot)
                        .with_scale(Vec3::splat(radius)),
                ));
            }
            BulletImpactSurface::Player | BulletImpactSurface::Npc => {
                // Blood feedback on character hits (NPCs + players)
                spawn_blood_splatter(
                    &mut commands,
                    &weapon_visuals,
                    &mut materials,
                    terrain.as_deref(),
                    impact.impact_position,
                    impact.impact_normal,
                    now,
                );
            }
        }

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
        transform.scale = Vec3::splat(marker.base_scale * scale);
        
        // Fade out the material
        if let Some(material) = materials.get_mut(&material_handle.0) {
            let base = marker.color.to_srgba();
            material.base_color = Color::srgba(base.red, base.green, base.blue, base.alpha * (1.0 - t));
        }
    }
}

// =============================================================================
// BLOOD SPLATTER EFFECTS
// =============================================================================

const BLOOD_GRAVITY: f32 = 12.0; // Gravity for blood droplets
const BLOOD_DROPLET_LIFETIME: f32 = 3.0; // Max time before despawn if never lands
const BLOOD_SPLAT_LIFETIME: f32 = 12.0; // How long ground splats last
const BLOOD_AIR_DRAG: f32 = 1.4; // Simple linear drag

// Blood burst settings (the instant PUBG-style hit indicator)
const BLOOD_BURST_LIFETIME: f32 = 0.25; // Very short! Punchy feedback
const BLOOD_BURST_INITIAL_SCALE: f32 = 0.15; // Start size
const BLOOD_BURST_MAX_SCALE: f32 = 0.6; // End size (expands 4x)

/// Spawn blood effects: instant burst for feedback + droplets for realism
fn spawn_blood_splatter(
    commands: &mut Commands,
    visuals: &WeaponVisualAssets,
    materials: &mut Assets<StandardMaterial>,
    _terrain: Option<&WorldTerrain>,
    impact_pos: Vec3,
    impact_normal: Vec3,
    now: f32,
) {
    let impact_normal = impact_normal.normalize_or_zero();
    let normal = if impact_normal.length_squared() > 1e-6 {
        impact_normal
    } else {
        Vec3::Y
    };

    // Use a simple pseudo-random based on position for variation
    let seed = (impact_pos.x * 1000.0 + impact_pos.z * 100.0 + impact_pos.y * 10.0) as i32;
    
    // =========================================================================
    // INSTANT BLOOD BURST (the key visual feedback!)
    // Multiple burst particles for a cloud effect
    // =========================================================================
    let num_bursts = 3 + (seed.abs() % 3) as usize; // 3-5 burst particles
    for i in 0..num_bursts {
        // Slight offset for each burst to create cloud effect
        let offset_angle = (i as f32 / num_bursts as f32) * std::f32::consts::TAU;
        let offset_dist = 0.05 + (((seed + i as i32) as f32 * 0.3).sin().abs()) * 0.1;
        let offset = Vec3::new(
            offset_angle.cos() * offset_dist,
            (((seed + i as i32 * 2) as f32 * 0.5).sin()) * 0.08,
            offset_angle.sin() * offset_dist,
        );
        
        let burst_pos = impact_pos + normal * 0.15 + offset;
        let scale_variation = 0.8 + (((seed + i as i32) as f32 * 0.7).sin().abs()) * 0.4;
        
        // Clone material for this burst so fading is independent
        let burst_mat = materials
            .get(&visuals.blood_burst_material)
            .cloned()
            .unwrap_or_else(|| StandardMaterial {
                base_color: Color::srgba(0.9, 0.1, 0.1, 0.9),
                emissive: LinearRgba::new(2.0, 0.2, 0.2, 1.0),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
        let burst_mat_handle = materials.add(burst_mat);
        
        commands.spawn((
            BloodBurst {
                spawn_time: now,
                lifetime: BLOOD_BURST_LIFETIME + (((seed + i as i32) as f32 * 0.4).sin().abs()) * 0.1,
                initial_scale: BLOOD_BURST_INITIAL_SCALE * scale_variation,
                max_scale: BLOOD_BURST_MAX_SCALE * scale_variation,
                direction: normal,
            },
            Mesh3d(visuals.blood_burst_mesh.clone()),
            MeshMaterial3d(burst_mat_handle),
            Transform::from_translation(burst_pos)
                .with_scale(Vec3::splat(BLOOD_BURST_INITIAL_SCALE * scale_variation)),
            Visibility::Visible,
            InheritedVisibility::default(),
        ));
    }
    
    // =========================================================================
    // FLYING DROPLETS (secondary - for realism, less important than burst)
    // =========================================================================
    let num_droplets = 4 + (seed.abs() % 3) as usize; // 4-6 droplets (reduced from before)
    for i in 0..num_droplets {
        let angle = (i as f32 / num_droplets as f32) * std::f32::consts::TAU 
            + ((seed + i as i32) as f32 * 0.3).sin() * 0.8;
        
        // Create tangent/bitangent for spray direction
        let tangent = if normal.y.abs() > 0.9 {
            Vec3::X
        } else {
            normal.cross(Vec3::Y).normalize_or_zero()
        };
        let bitangent = normal.cross(tangent).normalize_or_zero();
        
        // Spray direction: mostly outward from surface, with some upward component
        let horizontal_speed = 2.0 + (((seed + i as i32 * 7) as f32 * 0.5).sin().abs()) * 3.0;
        let vertical_speed = 1.5 + (((seed + i as i32 * 3) as f32 * 0.7).sin().abs()) * 2.5;
        
        let spray_dir = tangent * angle.cos() + bitangent * angle.sin();
        // Bias spray outward from the surface and a bit upward.
        let velocity = spray_dir * horizontal_speed + Vec3::Y * vertical_speed + normal * 1.2;
        
        // Small offset from impact point
        let droplet_pos = impact_pos + normal * 0.1 + spray_dir * 0.05;
        
        // Scale varies per droplet (small spheres)
        let scale = 0.04 + (((seed + i as i32 * 3) as f32 * 0.5).sin().abs()) * 0.06;
        
        commands.spawn((
            BloodDroplet {
                velocity,
                spawn_time: now,
            },
            Mesh3d(visuals.blood_droplet_mesh.clone()),
            MeshMaterial3d(visuals.blood_droplet_material.clone()),
            Transform::from_translation(droplet_pos)
                .with_scale(Vec3::splat(scale)),
            Visibility::Visible,
            InheritedVisibility::default(),
        ));
    }
}

/// Update flying blood droplets - apply gravity, check for ground collision
pub fn update_blood_droplets(
    mut commands: Commands,
    mut droplets: Query<(Entity, &mut BloodDroplet, &mut Transform)>,
    terrain: Option<Res<WorldTerrain>>,
    weapon_visuals: Option<Res<WeaponVisualAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    
    let Some(visuals) = weapon_visuals else { return };
    
    for (entity, mut droplet, mut transform) in droplets.iter_mut() {
        // Check lifetime
        let age = now - droplet.spawn_time;
        if age > BLOOD_DROPLET_LIFETIME {
            commands.entity(entity).despawn();
            continue;
        }
        
        // Apply gravity + simple drag
        droplet.velocity.y -= BLOOD_GRAVITY * dt;
        let v = droplet.velocity;
        droplet.velocity -= v * (BLOOD_AIR_DRAG * dt);
        
        // Move droplet
        let new_pos = transform.translation + droplet.velocity * dt;

        // Make droplets look like moving blobs (stretch along velocity)
        let speed = droplet.velocity.length();
        if speed > 0.2 {
            let dir = droplet.velocity / speed;
            transform.rotation = Quat::from_rotation_arc(Vec3::Y, dir);
            // Stretch more at higher speeds
            let base = transform.scale.x.max(0.01);
            let stretch = (1.0 + speed * 0.12).clamp(1.0, 2.2);
            transform.scale = Vec3::new(base * 0.7, base * stretch, base * 0.7);
        }
        
        // Check ground collision
        let ground_y = terrain
            .as_ref()
            .map(|t| t.generator.get_height(new_pos.x, new_pos.z))
            .unwrap_or(0.0);
        
        if new_pos.y <= ground_y + 0.02 {
            // Hit the ground! Spawn a ground splat and despawn the droplet
            let splat_pos = Vec3::new(new_pos.x, ground_y + 0.02, new_pos.z);
            
            // Splat size based on droplet size and speed
            let impact_speed = droplet.velocity.length();
            let base_scale = transform.scale.x.max(0.01);
            let splat_scale = (base_scale * 4.0 + impact_speed * 0.03).clamp(0.08, 0.55);
            
            // Direction smear based on impact velocity projected onto ground
            let dir_xz = Vec2::new(droplet.velocity.x, droplet.velocity.z);
            let dir_angle = dir_xz.y.atan2(dir_xz.x);
            let smear_rot = Quat::from_rotation_y(-dir_angle);
            let smear = (1.0 + impact_speed * 0.06).clamp(1.0, 2.8);

            // Create per-impact materials so fading one splat doesn't affect all others.
            // (Sharing a handle and mutating `StandardMaterial` caused flickering / “glitching”.)
            let base_splat_mat = materials
                .get(&visuals.blood_pool_material)
                .cloned()
                .unwrap_or_else(|| StandardMaterial {
                    base_color: Color::srgba(0.4, 0.01, 0.01, 0.85),
                    emissive: LinearRgba::new(0.2, 0.0, 0.0, 1.0),
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                });

            let splat_material = materials.add(base_splat_mat.clone());
            let ring_material = {
                let mut m = base_splat_mat;
                // Slightly lighter + more transparent for the quick splash ring.
                let c = m.base_color.to_srgba();
                m.base_color = Color::srgba(c.red * 1.05, c.green, c.blue, 0.55);
                m.emissive = LinearRgba::new(0.25, 0.0, 0.0, 1.0);
                materials.add(m)
            };

            // Main splat (slightly smeared)
            commands.spawn((
                BloodGroundSplat {
                    spawn_time: now,
                    lifetime: BLOOD_SPLAT_LIFETIME,
                    initial_scale: splat_scale,
                },
                Mesh3d(visuals.blood_splatter_mesh.clone()),
                MeshMaterial3d(splat_material.clone()),
                Transform::from_translation(splat_pos)
                    .with_rotation(smear_rot)
                    .with_scale(Vec3::new(splat_scale * smear, splat_scale, splat_scale)),
                Visibility::Visible,
                InheritedVisibility::default(),
            ));

            // Satellite droplets around the main splat (adds “splatter” texture without a texture)
            let seed = (new_pos.x * 120.0 + new_pos.z * 70.0) as i32;
            let satellites = 3 + (seed.abs() % 4) as usize; // 3-6
            for j in 0..satellites {
                let a = (j as f32 / satellites as f32) * std::f32::consts::TAU
                    + ((seed + j as i32) as f32 * 0.3).sin() * 0.8;
                let r = 0.08 + (((seed + j as i32 * 11) as f32 * 0.7).sin().abs()) * 0.25;
                let off = Vec3::new(a.cos() * r, 0.0, a.sin() * r);
                let s = (splat_scale * (0.25 + (((seed + j as i32 * 5) as f32 * 0.9).sin().abs()) * 0.35))
                    .clamp(0.03, 0.22);
                let rot = Quat::from_rotation_y(((seed + j as i32 * 13) as f32 * 0.17).sin() * std::f32::consts::TAU);
                commands.spawn((
                    BloodGroundSplat {
                        spawn_time: now,
                        lifetime: BLOOD_SPLAT_LIFETIME,
                        initial_scale: s,
                    },
                    Mesh3d(visuals.blood_splatter_mesh.clone()),
                    MeshMaterial3d(splat_material.clone()),
                    Transform::from_translation(splat_pos + off)
                        .with_rotation(rot)
                        .with_scale(Vec3::splat(s)),
                    Visibility::Visible,
                    InheritedVisibility::default(),
                ));
            }

            // Quick splash ring (expands and fades fast)
            commands.spawn((
                BloodSplashRing {
                    spawn_time: now,
                    lifetime: 0.35,
                    initial_scale: splat_scale * 0.9,
                },
                Mesh3d(visuals.blood_splatter_mesh.clone()),
                MeshMaterial3d(ring_material),
                Transform::from_translation(splat_pos)
                    .with_scale(Vec3::splat(splat_scale * 0.9)),
                Visibility::Visible,
                InheritedVisibility::default(),
            ));
            
            commands.entity(entity).despawn();
        } else {
            transform.translation = new_pos;
            
            // Shrink slightly as it flies (evaporation effect)
            transform.scale *= 1.0 - dt * 0.3;
        }
    }
}

/// Update splash rings - expand quickly then fade (ground-only)
pub fn update_blood_splash_rings(
    mut commands: Commands,
    mut rings: Query<(Entity, &BloodSplashRing, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();

    for (entity, ring, mut transform, material_handle) in rings.iter_mut() {
        let age = now - ring.spawn_time;
        if age > ring.lifetime {
            commands.entity(entity).despawn();
            continue;
        }

        let t = (age / ring.lifetime).clamp(0.0, 1.0);
        let expand = 1.0 + t * 1.8;
        transform.scale = Vec3::splat(ring.initial_scale * expand);

        if let Some(mat) = materials.get_mut(&material_handle.0) {
            let base = mat.base_color.to_srgba();
            // Fade aggressively; it’s just a quick “splash”
            let alpha = 0.55 * (1.0 - t);
            mat.base_color = Color::srgba(base.red, base.green, base.blue, alpha.max(0.0));
        }
    }
}

/// Update instant blood bursts - expand fast and fade (PUBG-style hit feedback)
pub fn update_blood_bursts(
    mut commands: Commands,
    mut bursts: Query<(Entity, &BloodBurst, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    
    for (entity, burst, mut transform, material_handle) in bursts.iter_mut() {
        let age = now - burst.spawn_time;
        
        if age > burst.lifetime {
            commands.entity(entity).despawn();
            continue;
        }
        
        // t goes 0->1 over the burst lifetime
        let t = (age / burst.lifetime).clamp(0.0, 1.0);
        
        // Expand quickly (ease-out curve for punchy feel)
        let ease_t = 1.0 - (1.0 - t).powi(2); // Quadratic ease-out
        let scale = burst.initial_scale + (burst.max_scale - burst.initial_scale) * ease_t;
        
        // Slight movement in the burst direction (blood "puffs" outward)
        let move_dist = ease_t * 0.15;
        let base_pos = transform.translation;
        transform.translation = base_pos + burst.direction * move_dist * time.delta_secs() * 10.0;
        
        // Scale up
        transform.scale = Vec3::splat(scale);
        
        // Fade out - starts visible, becomes transparent
        if let Some(mat) = materials.get_mut(&material_handle.0) {
            // Quick fade: fully visible at start, gone by end
            let alpha = (1.0 - t).powf(0.7) * 0.9; // Slightly aggressive fade
            let base = mat.base_color.to_srgba();
            mat.base_color = Color::srgba(base.red, base.green, base.blue, alpha.max(0.0));
            
            // Also fade emissive for less lingering glow
            let emissive_strength = (1.0 - t).powf(0.5) * 2.0;
            mat.emissive = LinearRgba::new(emissive_strength, emissive_strength * 0.1, emissive_strength * 0.1, 1.0);
        }
    }
}

/// Update ground blood splats - expand slightly then fade out
pub fn update_blood_ground_splats(
    mut commands: Commands,
    mut splats: Query<(Entity, &BloodGroundSplat, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    
    for (entity, splat, mut transform, material_handle) in splats.iter_mut() {
        let age = now - splat.spawn_time;
        
        if age > splat.lifetime {
            commands.entity(entity).despawn();
            continue;
        }
        
        let t = age / splat.lifetime;
        
        // Expand slightly in first 10% of lifetime (blood spreading)
        if t < 0.1 {
            let expand = 1.0 + (t / 0.1) * 0.3; // Expand up to 30%
            transform.scale = Vec3::splat(splat.initial_scale * expand);
        }
        
        // Fade out in the last 50% of lifetime
        if t > 0.5 {
            let fade_t = (t - 0.5) / 0.5;
            if let Some(mat) = materials.get_mut(&material_handle.0) {
                let base = mat.base_color.to_srgba();
                let alpha = 0.85 * (1.0 - fade_t);
                mat.base_color = Color::srgba(base.red, base.green, base.blue, alpha.max(0.0));
            }
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

            // Perf stats text (multi-line)
            parent.spawn((
                PerfStatsText,
                Text::new("Perf: --"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.85, 0.85)),
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
    mut perf_text_query: Query<&mut Text, (With<PerfStatsText>, Without<FpsText>)>,
    // Some useful counters (cheap to query)
    loaded_chunks: Res<crate::terrain::LoadedChunks>,
    props: Query<(), With<crate::props::EnvironmentProp>>,
    prop_kinds: Query<&crate::props::PropKindTag, With<crate::props::EnvironmentProp>>,
    collider_library: Option<Res<crate::props::ClientDerivedColliderLibrary>>,
    players: Query<&PlayerPosition, With<Player>>,
    bullets: Query<(), With<Bullet>>,
    local_tracers: Query<(), With<LocalTracer>>,
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

        // Build a compact perf readout:
        // - entity count
        // - chunk/prop/bullet counts
        // - top render CPU passes (if RenderDiagnosticsPlugin is enabled)
        let entity_count = diagnostics
            .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
            .and_then(|d| d.smoothed())
            .unwrap_or(0.0);

        // Collect top render/*/elapsed_cpu diagnostics
        let mut render_cpu: Vec<(&str, f64)> = diagnostics
            .iter()
            .filter_map(|d| {
                let path = d.path().as_str();
                if !path.starts_with("render/") || !path.ends_with("/elapsed_cpu") {
                    return None;
                }
                let v = d.smoothed()?;
                Some((path, v))
            })
            .collect();
        render_cpu.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut lines = String::new();
        let collidable_props = collider_library
            .as_ref()
            .map(|lib| {
                prop_kinds
                    .iter()
                    .filter(|k| lib.by_kind.contains_key(&k.0))
                    .count()
            })
            .unwrap_or(0);
        let baked_kinds = collider_library.as_ref().map(|lib| lib.by_kind.len()).unwrap_or(0);

        // Approximate the server-side collider chunk set (radius=3 chunks) by unioning all players.
        // This matches the server's current streaming radius in `server/src/colliders.rs`.
        let collider_chunk_radius = 3;
        let mut collider_chunks = std::collections::HashSet::new();
        for p in players.iter() {
            let center = ChunkCoord::from_world_pos(p.0);
            collider_chunks.extend(center.chunks_in_radius(collider_chunk_radius));
        }
        lines.push_str(&format!(
            "Entities: {:.0}\nChunks: {}\nProps: {}\nCollider chunks: {}\nCollidable props: {} (baked kinds: {})\nBullets: {} (local tracers: {})\n",
            entity_count,
            loaded_chunks.chunks.len(),
            props.iter().count(),
            collider_chunks.len(),
            collidable_props,
            baked_kinds,
            bullets.iter().count(),
            local_tracers.iter().count()
        ));

        if !render_cpu.is_empty() {
            lines.push_str("Render (CPU ms, top):\n");
            for (path, v) in render_cpu.iter().take(5) {
                // Show only the span name, not the whole prefix.
                let name = path
                    .strip_prefix("render/")
                    .unwrap_or(path)
                    .strip_suffix("/elapsed_cpu")
                    .unwrap_or(path);
                lines.push_str(&format!("  {name}: {v:.2}\n"));
            }
        } else {
            lines.push_str("Render: (enable RenderDiagnosticsPlugin)\n");
        }

        for mut text in perf_text_query.iter_mut() {
            text.0 = lines.clone();
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

// =============================================================================
// WEAPON VISUAL ASSET SETUP
// =============================================================================

/// Create shared meshes/materials for weapon visuals.
pub fn setup_weapon_visual_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Bullet tracer visual - a glowing elongated capsule
    let tracer_mesh = meshes.add(Capsule3d::new(0.08, 2.0));
    let tracer_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.8, 0.2),
        emissive: LinearRgba::new(20.0, 15.0, 5.0, 1.0),
        unlit: true,
        ..default()
    });

    // Impact marker disk - unit radius, scaled per marker.
    let impact_disk_mesh_unit = meshes.add(Cylinder::new(1.0, 0.03));

    // Blood splatter visuals
    let blood_splatter_mesh = meshes.add(Cylinder::new(1.0, 0.02)); // Thin disk for ground splats
    let blood_droplet_mesh = meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap()); // Small sphere for flying droplets
    let blood_burst_mesh = meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap()); // Larger sphere for burst
    
    // Dark red blood droplet material (for flying particles)
    let blood_droplet_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.6, 0.02, 0.02, 0.95),
        emissive: LinearRgba::new(0.4, 0.0, 0.0, 1.0), // Red glow so visible in air
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    
    // Darker blood pool material (for ground splats)
    // Use unlit + a touch of emissive so it stays visible even in deep shadow.
    let blood_pool_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.4, 0.01, 0.01, 0.85),
        emissive: LinearRgba::new(0.2, 0.0, 0.0, 1.0),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    
    // BRIGHT blood burst material for instant hit feedback (PUBG-style splat)
    // Very bright emissive so it's visible from distance
    let blood_burst_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.9, 0.1, 0.1, 0.9),
        emissive: LinearRgba::new(2.0, 0.2, 0.2, 1.0), // Strong red glow!
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    commands.insert_resource(WeaponVisualAssets {
        tracer_mesh,
        tracer_material,
        impact_disk_mesh_unit,
        blood_splatter_mesh,
        blood_droplet_mesh,
        blood_burst_mesh,
        blood_droplet_material,
        blood_pool_material,
        blood_burst_material,
    });
}
