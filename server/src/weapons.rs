//! Server-side weapon systems
//!
//! Handles bullet spawning, physics simulation, hit detection, and damage application.
//! Updated for Lightyear 0.25

use bevy::prelude::*;
use lightyear::prelude::*;
use lightyear::prelude::server::*;

use shared::{
    weapons::{ballistics, damage},
    Bullet, BulletPrevPosition, BulletVelocity, EquippedWeapon, Health,
    BulletImpact, BulletImpactSurface, HitConfirm, DamageReceived, PlayerKilled, ShootRequest, SwitchWeapon, ReloadRequest, ReliableChannel,
    AudioEvent, AudioEventKind,
    npc_capsule_endpoints, npc_head_center, Npc, NpcPosition, NpcDamageEvent, NPC_HEAD_RADIUS, NPC_HEIGHT, NPC_RADIUS,
    Player, PlayerPosition, WorldTerrain, FIXED_TIMESTEP_HZ, PLAYER_HEIGHT, PLAYER_RADIUS,
};

use crate::colliders::{DerivedColliderLibrary, StaticColliders, StructureColliders};

/// Server-only marker used to delay bullet despawn by a few frames.
///
/// This prevents clients from receiving a despawn for a bullet entity that was
/// spawned and destroyed within the same replication tick.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct BulletPendingDespawn {
    despawn_at: f32,
}

/// Helper to convert PeerId to u64 for owner tracking
fn peer_id_to_u64(peer_id: PeerId) -> u64 {
    match peer_id {
        PeerId::Netcode(id) => id,
        PeerId::Steam(id) => id,
        PeerId::Local(id) => id,
        PeerId::Entity(id) => id,
        PeerId::Raw(addr) => {
            // Hash the socket address to a u64
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            addr.hash(&mut hasher);
            hasher.finish()
        },
        PeerId::Server => 0,
    }
}

/// Handle shoot requests from clients
pub fn handle_shoot_requests(
    mut commands: Commands,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<ShootRequest>), With<ClientOf>>,
    mut players: Query<(&Player, &PlayerPosition, &mut EquippedWeapon)>,
    mut audio_senders: Query<&mut MessageSender<AudioEvent>, (With<ClientOf>, With<Connected>)>,
    time: Res<Time>,
) {
    let current_time = time.elapsed_secs();
    
    // Collect shots to broadcast audio events after processing
    let mut shots_fired: Vec<(u64, Vec3, shared::weapons::WeaponType)> = Vec::new();
    
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        
        for request in receiver.receive() {
            // Find the player who sent the request
            let Some((player, position, mut weapon)) = players.iter_mut().find(|(p, _, _)| p.client_id == peer_id) else {
                continue;
            };
            
            // Update aiming state
            weapon.aiming = request.aiming;
            
            // Check if can fire
            if !weapon.can_fire(current_time) {
                continue;
            }
            
            // Get weapon stats
            let stats = weapon.weapon_type.stats();
            
            // Fire the weapon (consumes ammo, updates cooldown)
            if !weapon.fire(current_time) {
                continue;
            }
            
            // Calculate spawn position at gun muzzle height
            let gun_height = PLAYER_HEIGHT * 0.29;
            let forward = request.direction.normalize();
            let right = forward.cross(Vec3::Y).normalize_or_zero();
            let spawn_offset = forward * 0.5 + right * 0.25;
            let spawn_pos = position.0 + Vec3::new(0.0, gun_height, 0.0) + spawn_offset;
            
            // Apply spread to direction
            let spread = weapon.current_spread();
            
            // Spawn bullets (multiple for shotgun)
            for _ in 0..stats.pellet_count {
                let spread_direction = ballistics::apply_spread(request.direction, spread);
                let velocity = spread_direction * stats.bullet_speed;
                
                commands.spawn((
                    Bullet {
                        owner_id: peer_id_to_u64(player.client_id),
                        weapon_type: weapon.weapon_type,
                        spawn_position: spawn_pos,
                        initial_velocity: velocity,
                        spawn_time: current_time,
                    },
                    BulletVelocity(velocity),
                    BulletPrevPosition(spawn_pos),
                    PlayerPosition(spawn_pos),
                    Transform::from_translation(spawn_pos),
                    Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                ));
            }
            
            // Record shot for audio broadcast
            shots_fired.push((peer_id_to_u64(player.client_id), spawn_pos, weapon.weapon_type));
            
            info!(
                "Player {:?} fired {:?} (ammo: {}/{})", 
                peer_id, weapon.weapon_type, weapon.ammo_in_mag, stats.magazine_size
            );
        }
    }
    
    // Broadcast audio events to all connected clients
    for (shooter_id, position, weapon_type) in shots_fired {
        let audio_event = AudioEvent {
            player_id: shooter_id,
            position,
            kind: AudioEventKind::Gunshot { weapon_type },
        };
        
        for mut sender in audio_senders.iter_mut() {
            sender.send::<ReliableChannel>(audio_event.clone());
        }
    }
}

/// Simulate bullet physics
pub fn simulate_bullets(
    mut bullets: Query<(
        &Bullet,
        &mut BulletVelocity,
        &mut BulletPrevPosition,
        &mut Transform,
        &mut PlayerPosition,
    ), Without<BulletPendingDespawn>>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
    
    for (_bullet, mut velocity, mut prev_pos, mut transform, mut position) in bullets.iter_mut() {
        prev_pos.0 = transform.translation;
        
        let (new_pos, new_vel) = ballistics::step_bullet_physics(
            transform.translation,
            velocity.0,
            dt,
        );
        
        transform.translation = new_pos;
        position.0 = new_pos;
        velocity.0 = new_vel;
    }
}

/// Detect bullet hits against players
pub fn detect_bullet_hits(
    mut commands: Commands,
    time: Res<Time>,
    bullets: Query<
        (Entity, &Bullet, &BulletVelocity, &BulletPrevPosition, &Transform),
        Without<BulletPendingDespawn>,
    >,
    mut players: Query<(Entity, &Player, &PlayerPosition, &mut Health), (With<Player>, Without<Npc>)>,
    mut npcs: Query<(Entity, &Npc, &NpcPosition, &mut Health), (With<Npc>, Without<Player>)>,
    mut client_links: Query<
        (
            &RemoteId,
            &mut MessageSender<HitConfirm>,
            &mut MessageSender<DamageReceived>,
            &mut MessageSender<PlayerKilled>,
            &mut MessageSender<BulletImpact>,
        ),
        (With<ClientOf>, With<Connected>),
    >,
) {
    // Collect hits first to avoid borrow issues
    #[derive(Clone, Copy, Debug)]
    enum Victim {
        Player(PeerId),
        Npc(Entity, u64),
    }

    #[derive(Clone, Copy, Debug)]
    struct HitRecord {
        bullet_entity: Entity,
        shooter_id: u64,
        victim: Victim,
        victim_pos: Vec3,
        hit_point: Vec3,
        hit_normal: Vec3,
        damage_amount: f32,
        hit_zone: damage::HitZone,
        weapon_type: shared::weapons::WeaponType,
        bullet_spawn_position: Vec3,
        bullet_initial_velocity: Vec3,
    }

    let now = time.elapsed_secs();
    let despawn_delay = 0.05;
    let mut hits: Vec<HitRecord> = Vec::new();
    
    for (bullet_entity, bullet, _velocity, prev_pos, transform) in bullets.iter() {
        let ray_start = prev_pos.0;
        let ray_end = transform.translation;
        let ray_dir = ray_end - ray_start;
        let ray_length = ray_dir.length();
        
        if ray_length < 0.001 {
            continue;
        }
        
        let ray_dir_norm = ray_dir / ray_length;

        let mut hit_recorded = false;
        
        // --- NPC hits (head sphere first, then capsule) ---
        for (npc_entity, npc, npc_pos, health) in npcs.iter() {
            if health.is_dead() {
                continue;
            }

            let head_center = npc_head_center(npc_pos.0);
            if let Some(hit_point) = ray_sphere_intersection(
                ray_start,
                ray_dir_norm,
                ray_length,
                head_center,
                NPC_HEAD_RADIUS,
            ) {
                let distance = (hit_point - bullet.spawn_position).length();
                let stats = bullet.weapon_type.stats();
                let damage_amount = damage::calculate_damage(&stats, distance, damage::HitZone::Head);

                let hit_normal = (hit_point - head_center).normalize_or_zero();
                hits.push(HitRecord {
                    bullet_entity,
                    shooter_id: bullet.owner_id,
                    victim: Victim::Npc(npc_entity, npc.id),
                    victim_pos: npc_pos.0,
                    hit_point,
                    hit_normal,
                    damage_amount,
                    hit_zone: damage::HitZone::Head,
                    weapon_type: bullet.weapon_type,
                    bullet_spawn_position: bullet.spawn_position,
                    bullet_initial_velocity: bullet.initial_velocity,
                });
                // One hit per bullet.
                hit_recorded = true;
                break;
            }

            let (a, b) = npc_capsule_endpoints(npc_pos.0);
            if let Some(hit_point) = ray_capsule_intersection(
                ray_start,
                ray_dir_norm,
                ray_length,
                a,
                b,
                NPC_RADIUS,
            ) {
                let bottom_y = npc_pos.0.y - NPC_HEIGHT * 0.5;
                let relative_height = (hit_point.y - bottom_y) / NPC_HEIGHT;
                let hit_zone = damage::HitZone::from_relative_height(relative_height);

                let distance = (hit_point - bullet.spawn_position).length();
                let stats = bullet.weapon_type.stats();
                let damage_amount = damage::calculate_damage(&stats, distance, hit_zone);

                // Approximate normal from capsule axis
                let ab = b - a;
                let t = if ab.length_squared() > 1e-6 {
                    (hit_point - a).dot(ab) / ab.length_squared()
                } else {
                    0.0
                }
                .clamp(0.0, 1.0);
                let closest = a + ab * t;
                let hit_normal = (hit_point - closest).normalize_or_zero();

                hits.push(HitRecord {
                    bullet_entity,
                    shooter_id: bullet.owner_id,
                    victim: Victim::Npc(npc_entity, npc.id),
                    victim_pos: npc_pos.0,
                    hit_point,
                    hit_normal,
                    damage_amount,
                    hit_zone,
                    weapon_type: bullet.weapon_type,
                    bullet_spawn_position: bullet.spawn_position,
                    bullet_initial_velocity: bullet.initial_velocity,
                });
                hit_recorded = true;
                break;
            }
        }

        if hit_recorded {
            continue;
        }

        // --- Player hits (existing capsule approximation) ---
        for (_player_entity, player, player_pos, health) in players.iter() {
            if peer_id_to_u64(player.client_id) == bullet.owner_id {
                continue;
            }

            if health.is_dead() {
                continue;
            }

            let capsule_bottom = player_pos.0;
            let capsule_top = player_pos.0 + Vec3::new(0.0, PLAYER_HEIGHT, 0.0);

            if let Some(hit_point) = ray_capsule_intersection(
                ray_start,
                ray_dir_norm,
                ray_length,
                capsule_bottom,
                capsule_top,
                PLAYER_RADIUS,
            ) {
                let relative_height = (hit_point.y - capsule_bottom.y) / PLAYER_HEIGHT;
                let hit_zone = damage::HitZone::from_relative_height(relative_height);
                let distance = (hit_point - bullet.spawn_position).length();
                let stats = bullet.weapon_type.stats();
                let damage_amount = damage::calculate_damage(&stats, distance, hit_zone);

                // Approximate normal from capsule axis
                let ab = capsule_top - capsule_bottom;
                let t = if ab.length_squared() > 1e-6 {
                    (hit_point - capsule_bottom).dot(ab) / ab.length_squared()
                } else {
                    0.0
                }
                .clamp(0.0, 1.0);
                let closest = capsule_bottom + ab * t;
                let hit_normal = (hit_point - closest).normalize_or_zero();

                hits.push(HitRecord {
                    bullet_entity,
                    shooter_id: bullet.owner_id,
                    victim: Victim::Player(player.client_id),
                    victim_pos: player_pos.0,
                    hit_point,
                    hit_normal,
                    damage_amount,
                    hit_zone,
                    weapon_type: bullet.weapon_type,
                    bullet_spawn_position: bullet.spawn_position,
                    bullet_initial_velocity: bullet.initial_velocity,
                });
                break;
            }
        }
    }
    
    // Collect shooter peer IDs
    let shooter_ids: std::collections::HashMap<u64, PeerId> = players
        .iter()
        .map(|(_, p, _, _)| (peer_id_to_u64(p.client_id), p.client_id))
        .collect();
    
    // Process hits
    for hit in hits {
        let shooter_peer_id = shooter_ids.get(&hit.shooter_id).copied();
        
        match hit.victim {
            Victim::Player(victim_id) => {
                // Find and damage the victim
                for (_, player, _, mut health) in players.iter_mut() {
                    if player.client_id == victim_id {
                        let is_kill = health.take_damage(hit.damage_amount);
                        let is_headshot = hit.hit_zone == damage::HitZone::Head;

                        info!(
                            "Hit! {:?} -> {:?} ({:?}) for {:.1} damage (headshot: {}, kill: {})",
                            hit.shooter_id, victim_id, hit.hit_zone, hit.damage_amount, is_headshot, is_kill
                        );

                        let impact = BulletImpact {
                            owner_id: hit.shooter_id,
                            weapon_type: hit.weapon_type,
                            spawn_position: hit.bullet_spawn_position,
                            initial_velocity: hit.bullet_initial_velocity,
                            impact_position: hit.hit_point,
                            impact_normal: hit.hit_normal,
                            surface: BulletImpactSurface::Player,
                        };

                        // Send messages via MessageSender components
                        for (remote_id, mut hit_sender, mut dmg_sender, mut kill_sender, mut impact_sender) in
                            client_links.iter_mut()
                        {
                            // Always send impact so everyone can render hit effects.
                            impact_sender.send::<ReliableChannel>(impact.clone());

                            // Send hit confirm to shooter
                            if let Some(sid) = shooter_peer_id {
                                if remote_id.0 == sid {
                                    hit_sender.send::<ReliableChannel>(HitConfirm {
                                        target_id: peer_id_to_u64(victim_id),
                                        damage: hit.damage_amount,
                                        headshot: is_headshot,
                                        kill: is_kill,
                                        hit_zone: hit.hit_zone,
                                    });
                                }
                            }

                            // Send damage notification to victim
                            if remote_id.0 == victim_id {
                                let damage_direction = Vec3::new(
                                    hit.bullet_spawn_position.x - hit.victim_pos.x,
                                    0.0,
                                    hit.bullet_spawn_position.z - hit.victim_pos.z,
                                )
                                .normalize_or_zero();
                                dmg_sender.send::<ReliableChannel>(DamageReceived {
                                    direction: damage_direction,
                                    damage: hit.damage_amount,
                                    health_remaining: health.current,
                                });

                                if is_kill {
                                    kill_sender.send::<ReliableChannel>(PlayerKilled {
                                        killer_id: hit.shooter_id,
                                        weapon: hit.weapon_type,
                                        headshot: is_headshot,
                                    });
                                }
                            }
                        }

                        break;
                    }
                }
            }
            Victim::Npc(npc_entity, npc_id) => {
                if let Ok((_e, _npc, _pos, mut health)) = npcs.get_mut(npc_entity) {
                    let is_kill = health.take_damage(hit.damage_amount);
                    let is_headshot = hit.hit_zone == damage::HitZone::Head;

                    // Add damage event component so AI can react
                    commands.entity(npc_entity).insert(NpcDamageEvent {
                        damage_source_position: hit.bullet_spawn_position,
                        damage_amount: hit.damage_amount,
                    });

                    info!(
                        "Hit NPC! {:?} -> npc:{} ({:?}) for {:.1} damage (headshot: {}, kill: {})",
                        hit.shooter_id, npc_id, hit.hit_zone, hit.damage_amount, is_headshot, is_kill
                    );

                    let impact = BulletImpact {
                        owner_id: hit.shooter_id,
                        weapon_type: hit.weapon_type,
                        spawn_position: hit.bullet_spawn_position,
                        initial_velocity: hit.bullet_initial_velocity,
                        impact_position: hit.hit_point,
                        impact_normal: hit.hit_normal,
                        surface: BulletImpactSurface::Npc,
                    };

                    // Send hit confirm to shooter only.
                    for (remote_id, mut hit_sender, _dmg_sender, _kill_sender, mut impact_sender) in
                        client_links.iter_mut()
                    {
                        // Everyone gets the impact for visuals (blood, etc.)
                        impact_sender.send::<ReliableChannel>(impact.clone());

                        if let Some(sid) = shooter_peer_id {
                            if remote_id.0 == sid {
                                hit_sender.send::<ReliableChannel>(HitConfirm {
                                    target_id: npc_id,
                                    damage: hit.damage_amount,
                                    headshot: is_headshot,
                                    kill: is_kill,
                                    hit_zone: hit.hit_zone,
                                });
                            }
                        }
                    }
                }
            }
        }
        
        // Delay despawn to avoid replication "despawn for entity that does not exist" errors.
        commands.entity(hit.bullet_entity).insert((
            BulletPendingDespawn {
                despawn_at: now + despawn_delay,
            },
            Transform::from_translation(hit.hit_point),
            PlayerPosition(hit.hit_point),
        ));
    }
}

/// Ray-sphere intersection (approx): returns closest point along ray segment if within radius.
fn ray_sphere_intersection(
    ray_origin: Vec3,
    ray_dir: Vec3,
    ray_length: f32,
    sphere_center: Vec3,
    sphere_radius: f32,
) -> Option<Vec3> {
    let to_center = sphere_center - ray_origin;
    let t = to_center.dot(ray_dir).clamp(0.0, ray_length);
    let p = ray_origin + ray_dir * t;
    let d = (p - sphere_center).length();
    let effective = sphere_radius * 1.25;
    (d <= effective).then_some(p)
}

/// Detect bullet hits against world geometry (terrain, practice wall, props, structures)
pub fn detect_bullet_world_hits(
    mut commands: Commands,
    time: Res<Time>,
    bullets: Query<(Entity, &Bullet, &BulletPrevPosition, &Transform), Without<BulletPendingDespawn>>,
    terrain: Res<WorldTerrain>,
    _players: Query<&Player>,
    derived_colliders: Option<Res<DerivedColliderLibrary>>,
    static_colliders: Res<StaticColliders>,
    structure_colliders: Res<StructureColliders>,
    mut client_links: Query<(&RemoteId, &mut MessageSender<BulletImpact>), (With<ClientOf>, With<Connected>)>,
) {
    // Wall is 50m north of spawn, facing south
    const WALL_X: f32 = 0.0;
    const WALL_Z: f32 = 50.0;
    const WALL_WIDTH: f32 = 20.0;
    const WALL_HEIGHT: f32 = 10.0;
    const WALL_THICKNESS: f32 = 1.0;

    let wall_ground_y = terrain.get_height(WALL_X, WALL_Z);
    let wall_min = Vec3::new(
        WALL_X - WALL_WIDTH * 0.5,
        wall_ground_y,
        WALL_Z - WALL_THICKNESS * 0.5,
    );
    let wall_max = Vec3::new(
        WALL_X + WALL_WIDTH * 0.5,
        wall_ground_y + WALL_HEIGHT,
        WALL_Z + WALL_THICKNESS * 0.5,
    );

    let now = time.elapsed_secs();
    let despawn_delay = 0.05;

    for (bullet_entity, bullet, prev_pos, transform) in bullets.iter() {
        let start = prev_pos.0;
        let end = transform.translation;
        let dir = end - start;

        if dir.length_squared() < 1e-6 {
            continue;
        }

        let mut best_hit: Option<(f32, Vec3, Vec3, BulletImpactSurface)> = None;

        // Check practice wall
        if let Some((t, hit_point, hit_normal)) = segment_aabb_intersection(start, end, wall_min, wall_max) {
            best_hit = Some((t, hit_point, hit_normal, BulletImpactSurface::PracticeWall));
        }

        // Check terrain
        if let Some((t, hit_point, hit_normal)) = segment_terrain_intersection(&terrain, start, end) {
            match best_hit {
                Some((best_t, _, _, _)) if best_t <= t => {}
                _ => best_hit = Some((t, hit_point, hit_normal, BulletImpactSurface::Terrain)),
            }
        }

        // Check static props (trees, rocks, etc.)
        if let Some(ref derived) = derived_colliders {
            if let Some((t, hit_point, hit_normal)) = segment_props_intersection(
                start, end, &static_colliders, derived,
            ) {
                match best_hit {
                    Some((best_t, _, _, _)) if best_t <= t => {}
                    _ => best_hit = Some((t, hit_point, hit_normal, BulletImpactSurface::Terrain)), // Use Terrain surface for props
                }
            }
        }

        // Check structures (domes, walls, towers, etc.)
        if let Some((t, hit_point, hit_normal)) = segment_structures_intersection(
            start, end, &structure_colliders,
        ) {
            match best_hit {
                Some((best_t, _, _, _)) if best_t <= t => {}
                _ => best_hit = Some((t, hit_point, hit_normal, BulletImpactSurface::PracticeWall)), // Use wall surface for structures
            }
        }

        if let Some((_t, hit_point, hit_normal, surface)) = best_hit {
            let impact = BulletImpact {
                owner_id: bullet.owner_id,
                weapon_type: bullet.weapon_type,
                spawn_position: bullet.spawn_position,
                initial_velocity: bullet.initial_velocity,
                impact_position: hit_point,
                impact_normal: hit_normal,
                surface,
            };

            // Send to all clients
            for (_remote_id, mut sender) in client_links.iter_mut() {
                sender.send::<ReliableChannel>(impact.clone());
            }

            // Delay despawn to avoid replication "despawn for entity that does not exist" errors.
            commands.entity(bullet_entity).insert((
                BulletPendingDespawn {
                    despawn_at: now + despawn_delay,
                },
                Transform::from_translation(hit_point),
                PlayerPosition(hit_point),
            ));
        }
    }
}

/// Clean up bullets that are out of bounds or expired
pub fn cleanup_bullets(
    mut commands: Commands,
    bullets: Query<(Entity, &Bullet, &BulletVelocity, &Transform, Option<&BulletPendingDespawn>)>,
    time: Res<Time>,
) {
    let current_time = time.elapsed_secs();
    
    for (entity, bullet, velocity, transform, pending) in bullets.iter() {
        if let Some(pending) = pending {
            if current_time >= pending.despawn_at {
                commands.entity(entity).despawn();
            }
            continue;
        }

        if ballistics::should_despawn_bullet(
            velocity.0,
            bullet.spawn_position,
            transform.translation,
            bullet.spawn_time,
            current_time,
        ) {
            commands.entity(entity).despawn();
        }
        
        if transform.translation.y < -50.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Ray-capsule intersection test
fn ray_capsule_intersection(
    ray_origin: Vec3,
    ray_dir: Vec3,
    ray_length: f32,
    capsule_a: Vec3,
    capsule_b: Vec3,
    capsule_radius: f32,
) -> Option<Vec3> {
    let capsule_center = (capsule_a + capsule_b) * 0.5;
    let capsule_half_height = (capsule_b.y - capsule_a.y) * 0.5;
    
    let to_center = capsule_center - ray_origin;
    let closest_t = to_center.dot(ray_dir).clamp(0.0, ray_length);
    let closest_point = ray_origin + ray_dir * closest_t;
    
    let effective_radius = capsule_radius * 1.5;
    
    let horizontal_dist = Vec2::new(
        closest_point.x - capsule_center.x,
        closest_point.z - capsule_center.z,
    ).length();
    
    if horizontal_dist > effective_radius {
        return None;
    }
    
    let height_diff = closest_point.y - capsule_center.y;
    if height_diff.abs() > capsule_half_height + effective_radius {
        return None;
    }
    
    Some(closest_point)
}

/// Segment vs AABB intersection
fn segment_aabb_intersection(
    start: Vec3,
    end: Vec3,
    aabb_min: Vec3,
    aabb_max: Vec3,
) -> Option<(f32, Vec3, Vec3)> {
    let dir = end - start;
    let mut tmin = 0.0_f32;
    let mut tmax = 1.0_f32;
    let mut hit_normal = Vec3::ZERO;

    for axis in 0..3 {
        let s = start[axis];
        let d = dir[axis];
        let min = aabb_min[axis];
        let max = aabb_max[axis];

        if d.abs() < 1e-6 {
            if s < min || s > max {
                return None;
            }
            continue;
        }

        let inv_d = 1.0 / d;
        let mut t1 = (min - s) * inv_d;
        let mut t2 = (max - s) * inv_d;

        let mut n = Vec3::ZERO;
        n[axis] = if d > 0.0 { -1.0 } else { 1.0 };

        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
            n = -n;
        }

        if t1 > tmin {
            tmin = t1;
            hit_normal = n;
        }

        tmax = tmax.min(t2);

        if tmin > tmax {
            return None;
        }
    }

    if !(0.0..=1.0).contains(&tmin) {
        return None;
    }

    let hit_point = start + dir * tmin;
    Some((tmin, hit_point, hit_normal))
}

/// Segment vs terrain heightfield intersection
fn segment_terrain_intersection(
    terrain: &WorldTerrain,
    start: Vec3,
    end: Vec3,
) -> Option<(f32, Vec3, Vec3)> {
    let dir = end - start;
    let length = dir.length();
    if length < 1e-3 {
        return None;
    }

    let f = |p: Vec3| -> f32 { p.y - terrain.get_height(p.x, p.z) };

    if f(start) <= 0.0 {
        let ground_y = terrain.get_height(start.x, start.z);
        let hit_pos = Vec3::new(start.x, ground_y, start.z);
        let normal = terrain.get_normal(hit_pos.x, hit_pos.z);
        return Some((0.0, hit_pos, normal));
    }

    let step_size = 0.5_f32;
    let steps = (length / step_size).ceil().clamp(1.0, 200.0) as u32;

    let mut prev_t = 0.0_f32;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let p = start + dir * t;
        if f(p) <= 0.0 {
            let mut lo = prev_t;
            let mut hi = t;
            for _ in 0..12 {
                let mid = (lo + hi) * 0.5;
                let pmid = start + dir * mid;
                if f(pmid) > 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }

            let t_hit = hi;
            let p_hit = start + dir * t_hit;
            let ground_y = terrain.get_height(p_hit.x, p_hit.z);
            let hit_pos = Vec3::new(p_hit.x, ground_y, p_hit.z);
            let normal = terrain.get_normal(hit_pos.x, hit_pos.z);
            return Some((t_hit, hit_pos, normal));
        }
        prev_t = t;
    }

    None
}

/// Test ray segment against static props (trees, rocks, etc.)
/// Returns (t, hit_point, hit_normal) for the closest hit
fn segment_props_intersection(
    start: Vec3,
    end: Vec3,
    colliders: &StaticColliders,
    derived: &DerivedColliderLibrary,
) -> Option<(f32, Vec3, Vec3)> {
    let dir = end - start;
    let length = dir.length();
    if length < 1e-4 {
        return None;
    }
    let ray_dir = dir / length;

    let mut best_hit: Option<(f32, Vec3, Vec3)> = None;

    // Get midpoint for spatial query
    let mid = (start + end) * 0.5;
    let query_radius = length * 0.5 + 5.0; // Extra margin for large props

    // Find nearby props using spatial hash
    let cell_size = 16.0; // Must match COLLIDER_CELL_SIZE in colliders.rs
    let cx = (mid.x / cell_size).floor() as i32;
    let cz = (mid.z / cell_size).floor() as i32;
    let cells = (query_radius / cell_size).ceil() as i32 + 1;

    for dx in -cells..=cells {
        for dz in -cells..=cells {
            let Some(ids) = colliders.cells.get(&(cx + dx, cz + dz)) else {
                continue;
            };

            for &id in ids {
                let Some(inst) = colliders.instances.get(&id) else {
                    continue;
                };
                let Some(shape) = derived.by_kind.get(&inst.kind) else {
                    continue;
                };

                // Broad phase: bounding sphere
                let bounding_r = shape.bounding_radius * inst.scale;
                let to_prop = mid - inst.position;
                if to_prop.length() > query_radius + bounding_r {
                    continue;
                }

                // Test ray against each face of the hull
                for face in &shape.hull_faces {
                    // Transform face vertices to world space
                    let v0 = inst.position + inst.rotation * (face.vertices[0] * inst.scale);
                    let v1 = inst.position + inst.rotation * (face.vertices[1] * inst.scale);
                    let v2 = inst.position + inst.rotation * (face.vertices[2] * inst.scale);
                    let world_normal = inst.rotation * face.normal;

                    // Ray-triangle intersection (Möller–Trumbore)
                    if let Some((t, hit_point)) = ray_triangle_intersection(
                        start, ray_dir, length, v0, v1, v2,
                    ) {
                        match best_hit {
                            Some((best_t, _, _)) if best_t <= t => {}
                            _ => best_hit = Some((t, hit_point, world_normal)),
                        }
                    }
                }
            }
        }
    }

    best_hit
}

/// Möller–Trumbore ray-triangle intersection
fn ray_triangle_intersection(
    ray_origin: Vec3,
    ray_dir: Vec3,
    max_t: f32,
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
) -> Option<(f32, Vec3)> {
    const EPSILON: f32 = 1e-6;

    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let h = ray_dir.cross(edge2);
    let a = edge1.dot(h);

    if a.abs() < EPSILON {
        return None; // Ray is parallel to triangle
    }

    let f = 1.0 / a;
    let s = ray_origin - v0;
    let u = f * s.dot(h);

    if u < 0.0 || u > 1.0 {
        return None;
    }

    let q = s.cross(edge1);
    let v = f * ray_dir.dot(q);

    if v < 0.0 || u + v > 1.0 {
        return None;
    }

    let t = f * edge2.dot(q);

    if t > EPSILON && t <= max_t {
        let hit_point = ray_origin + ray_dir * t;
        Some((t, hit_point))
    } else {
        None
    }
}

/// Check ray intersection against structures (domes, walls, towers, etc.)
fn segment_structures_intersection(
    start: Vec3,
    end: Vec3,
    colliders: &StructureColliders,
) -> Option<(f32, Vec3, Vec3)> {
    use shared::StructureCollider;
    
    let dir = end - start;
    let length = dir.length();
    if length < 1e-4 {
        return None;
    }
    let ray_dir = dir / length;

    let mut best_hit: Option<(f32, Vec3, Vec3)> = None;

    // Get midpoint for spatial query
    let mid = (start + end) * 0.5;
    let query_radius = length * 0.5 + 15.0; // Extra margin for large structures

    // Find nearby structures using spatial hash
    let cell_size = 16.0;
    let cx = (mid.x / cell_size).floor() as i32;
    let cz = (mid.z / cell_size).floor() as i32;
    let cells = (query_radius / cell_size).ceil() as i32 + 1;

    for dx in -cells..=cells {
        for dz in -cells..=cells {
            let Some(ids) = colliders.cells.get(&(cx + dx, cz + dz)) else {
                continue;
            };

            for &id in ids {
                let Some(inst) = colliders.instances.get(&id) else {
                    continue;
                };

                let collider = inst.kind.collider();
                let pos = inst.position;
                let scale = inst.scale;

                // Test ray against structure based on its collision shape
                let hit = match collider {
                    StructureCollider::Dome { radius, height } => {
                        ray_dome_intersection(start, ray_dir, length, pos, radius * scale, height * scale)
                    }
                    StructureCollider::Cylinder { radius, height } => {
                        ray_cylinder_intersection(start, ray_dir, length, pos, radius * scale, height * scale)
                    }
                    StructureCollider::Box { half_extents } => {
                        let scaled = half_extents * scale;
                        let min = pos - Vec3::new(scaled.x, 0.0, scaled.z);
                        let max = pos + Vec3::new(scaled.x, scaled.y * 2.0, scaled.z);
                        segment_aabb_intersection(start, end, min, max)
                    }
                    StructureCollider::Arch { width, height, depth, thickness } => {
                        // Approximate arch as two pillars + top box
                        let hw = width * 0.5 * scale;
                        let hd = depth * 0.5 * scale;
                        let th = thickness * scale;
                        let h = height * scale;
                        
                        // Left pillar
                        let left_min = pos + Vec3::new(-hw, 0.0, -hd);
                        let left_max = pos + Vec3::new(-hw + th, h * 0.7, hd);
                        if let Some(hit) = segment_aabb_intersection(start, end, left_min, left_max) {
                            Some(hit)
                        } else {
                            // Right pillar
                            let right_min = pos + Vec3::new(hw - th, 0.0, -hd);
                            let right_max = pos + Vec3::new(hw, h * 0.7, hd);
                            if let Some(hit) = segment_aabb_intersection(start, end, right_min, right_max) {
                                Some(hit)
                            } else {
                                // Top span
                                let top_min = pos + Vec3::new(-hw, h * 0.6, -hd);
                                let top_max = pos + Vec3::new(hw, h, hd);
                                segment_aabb_intersection(start, end, top_min, top_max)
                            }
                        }
                    }
                };

                if let Some((t, hit_point, hit_normal)) = hit {
                    match best_hit {
                        Some((best_t, _, _)) if best_t <= t => {}
                        _ => best_hit = Some((t, hit_point, hit_normal)),
                    }
                }
            }
        }
    }

    best_hit
}

/// Ray-dome intersection (hemisphere)
fn ray_dome_intersection(
    ray_origin: Vec3,
    ray_dir: Vec3,
    max_t: f32,
    dome_pos: Vec3,
    radius: f32,
    height: f32,
) -> Option<(f32, Vec3, Vec3)> {
    // Scale Y to make it a hemisphere check
    let height_ratio = height / radius;
    let scaled_origin = Vec3::new(
        ray_origin.x - dome_pos.x,
        (ray_origin.y - dome_pos.y) / height_ratio,
        ray_origin.z - dome_pos.z,
    );
    let scaled_dir = Vec3::new(ray_dir.x, ray_dir.y / height_ratio, ray_dir.z).normalize();
    
    // Sphere intersection in scaled space
    let a = scaled_dir.dot(scaled_dir);
    let b = 2.0 * scaled_origin.dot(scaled_dir);
    let c = scaled_origin.dot(scaled_origin) - radius * radius;
    
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return None;
    }
    
    let sqrt_d = discriminant.sqrt();
    let t1 = (-b - sqrt_d) / (2.0 * a);
    let t2 = (-b + sqrt_d) / (2.0 * a);
    
    for t_scaled in [t1, t2] {
        if t_scaled > 0.001 {
            // Convert back to world space
            let hit_scaled = scaled_origin + scaled_dir * t_scaled;
            let hit_world = Vec3::new(
                hit_scaled.x + dome_pos.x,
                hit_scaled.y * height_ratio + dome_pos.y,
                hit_scaled.z + dome_pos.z,
            );
            
            // Check if above ground (dome is upper hemisphere)
            if hit_world.y >= dome_pos.y {
                let t_world = (hit_world - ray_origin).dot(ray_dir);
                if t_world > 0.001 && t_world <= max_t {
                    let normal = Vec3::new(
                        hit_scaled.x,
                        hit_scaled.y / height_ratio,
                        hit_scaled.z,
                    ).normalize();
                    return Some((t_world, hit_world, normal));
                }
            }
        }
    }
    
    None
}

/// Ray-cylinder intersection
fn ray_cylinder_intersection(
    ray_origin: Vec3,
    ray_dir: Vec3,
    max_t: f32,
    cyl_pos: Vec3,
    radius: f32,
    height: f32,
) -> Option<(f32, Vec3, Vec3)> {
    // Cylinder is at cyl_pos with base on ground
    let local_origin = ray_origin - cyl_pos;
    
    // 2D circle intersection in XZ plane
    let a = ray_dir.x * ray_dir.x + ray_dir.z * ray_dir.z;
    let b = 2.0 * (local_origin.x * ray_dir.x + local_origin.z * ray_dir.z);
    let c = local_origin.x * local_origin.x + local_origin.z * local_origin.z - radius * radius;
    
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 || a.abs() < 1e-6 {
        return None;
    }
    
    let sqrt_d = discriminant.sqrt();
    let t1 = (-b - sqrt_d) / (2.0 * a);
    let t2 = (-b + sqrt_d) / (2.0 * a);
    
    for t in [t1, t2] {
        if t > 0.001 && t <= max_t {
            let hit_point = ray_origin + ray_dir * t;
            let local_hit = hit_point - cyl_pos;
            
            // Check height bounds
            if local_hit.y >= 0.0 && local_hit.y <= height {
                let normal = Vec3::new(local_hit.x, 0.0, local_hit.z).normalize();
                return Some((t, hit_point, normal));
            }
        }
    }
    
    // Check top cap
    if ray_dir.y.abs() > 1e-6 {
        let t_top = (cyl_pos.y + height - ray_origin.y) / ray_dir.y;
        if t_top > 0.001 && t_top <= max_t {
            let hit_point = ray_origin + ray_dir * t_top;
            let local_hit = hit_point - cyl_pos;
            let dist_sq = local_hit.x * local_hit.x + local_hit.z * local_hit.z;
            if dist_sq <= radius * radius {
                return Some((t_top, hit_point, Vec3::Y));
            }
        }
    }

    None
}

/// Handle weapon switch requests from clients
#[allow(dead_code)]
pub fn handle_weapon_switch(
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<SwitchWeapon>), With<ClientOf>>,
    mut players: Query<(&Player, &mut EquippedWeapon)>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        
        for request in receiver.receive() {
            for (player, mut weapon) in players.iter_mut() {
                if player.client_id == peer_id {
                    let stats = request.weapon_type.stats();
                    weapon.weapon_type = request.weapon_type;
                    weapon.ammo_in_mag = stats.magazine_size;
                    weapon.reserve_ammo = stats.magazine_size * 3;
                    weapon.last_fire_time = -10.0;
                    
                    info!("Player {:?} switched to {:?}", peer_id, request.weapon_type);
                    break;
                }
            }
        }
    }
}

/// Handle reload requests from clients
pub fn handle_reload_request(
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<ReloadRequest>), With<ClientOf>>,
    mut players: Query<(&Player, &mut EquippedWeapon, &mut shared::Inventory)>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        
        for _msg in receiver.receive() {
            for (player, mut weapon, mut inventory) in players.iter_mut() {
                if player.client_id == peer_id {
                    let stats = weapon.weapon_type.stats();
                    let ammo_type = weapon.weapon_type.ammo_type();
                    let needed = stats.magazine_size - weapon.ammo_in_mag;
                    let reserve_in_inventory = inventory.count_item(ammo_type);
                    
                    if needed > 0 && reserve_in_inventory > 0 {
                        // Take ammo from inventory
                        let taken = weapon.reload_from_inventory(&mut inventory);
                        
                        info!(
                            "Player {:?} reloaded {:?} (took {} ammo): {}/{} (reserve in inventory: {})", 
                            peer_id, weapon.weapon_type, taken, weapon.ammo_in_mag, stats.magazine_size, 
                            inventory.count_item(ammo_type)
                        );
                    }
                    break;
                }
            }
        }
    }
}
