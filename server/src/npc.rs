//! Server-side NPC + AI systems.
//!
//! Goals for v1:
//! - Spawn a simple damageable NPC near the player spawn.
//! - Wander around using lightweight grid A* pathfinding over the heightfield terrain.
//! - Server-authoritative position/rotation replicated to clients.

use bevy::prelude::*;
use lightyear::prelude::*;
use lightyear::prelude::server::Started;

use shared::{
    ground_clearance_center, npc_capsule_endpoints, npc_head_center, Npc, NpcArchetype, NpcPosition,
    NpcRotation, NpcDamageEvent, WorldTerrain, FIXED_TIMESTEP_HZ, Health,
    PlacedBuilding, BuildingPosition,
    SpatialObstacleGrid, ObstacleEntry,
    // NPC constants from shared
    npc_max_health, NPC_MOVE_SPEED, NPC_TURN_SPEED, NPC_WANDER_RADIUS,
    NPC_IDLE_TIME_MIN, NPC_IDLE_TIME_MAX, NPC_MIN_TARGET_DIST, DEAD_NPC_DESPAWN_TIME,
};

// =============================================================================
// SPATIAL GRID (obstacle caching for O(1) lookups)
// =============================================================================

/// Tracks the last known count of buildings to detect changes.
#[derive(Resource, Default)]
pub struct ObstacleGridState {
    pub last_building_count: usize,
}

/// Sync the SpatialObstacleGrid with current buildings.
/// Only rebuilds when buildings are added/removed (not every frame).
pub fn sync_obstacle_grid(
    mut grid: ResMut<SpatialObstacleGrid>,
    mut state: ResMut<ObstacleGridState>,
    buildings: Query<(&PlacedBuilding, &BuildingPosition)>,
) {
    let current_count = buildings.iter().count();

    // Only rebuild if building count changed
    if current_count == state.last_building_count && !grid.is_empty() {
        return;
    }

    state.last_building_count = current_count;
    grid.clear();

    for (building, pos) in buildings.iter() {
        let def = building.building_type.definition();
        let half_extents = Vec2::new(
            def.footprint.x / 2.0 + def.flatten_radius,
            def.footprint.y / 2.0 + def.flatten_radius,
        );

        grid.insert(ObstacleEntry {
            center: Vec2::new(pos.0.x, pos.0.z),
            half_extents,
            rotation: building.rotation,
            obstacle_type: building.building_type as u32,
        });
    }

    if current_count > 0 {
        trace!("Rebuilt spatial grid with {} obstacles", current_count);
    }
}

// =============================================================================
// SPAWN
// =============================================================================

/// One-shot resource to ensure NPCs are only spawned once.
#[derive(Resource)]
pub struct NpcsSpawned;

/// Timer component for tracking how long an NPC has been dead.
/// When the timer reaches 0, the NPC entity will be despawned.
#[derive(Component)]
pub struct DeadNpcDespawnTimer(pub f32);

/// Spawn NPCs near the player spawn once the server is started.
pub fn spawn_npcs_once(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    spawned: Option<Res<NpcsSpawned>>,
    // The `Started` component is present on the server entity once networking is up.
    server_started: Query<(), With<Started>>,
) {
    if spawned.is_some() || server_started.is_empty() {
        return;
    }
    commands.insert_resource(NpcsSpawned);

    // Spawn 3 barbarian NPCs at different positions around the player spawn
    let barbarian_positions = [
        (6.0, -4.0),   // Original position
        (-8.0, -6.0),  // Left side
        (4.0, 10.0),   // Behind/south
    ];

    for (npc_id, (x, z)) in barbarian_positions.iter().enumerate() {
        let npc_id = (npc_id + 1) as u64;
        let y = terrain.get_height(*x, *z) + ground_clearance_center();
        let pos = Vec3::new(*x, y, *z);

        commands.spawn((
            Npc {
                id: npc_id,
                archetype: NpcArchetype::Barbarian,
            },
            NpcPosition(pos),
            NpcRotation(0.0),
            Health::new(npc_max_health(NpcArchetype::Barbarian)),
            NpcWander::new(pos, 18.0, npc_id),
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ));

        trace!("Spawned Barbarian NPC {} at {:?}", npc_id, pos);
    }

    // Spawn Knight NPC (dialogue NPC, smaller wander radius)
    let knight_pos = Vec3::new(12.0, terrain.get_height(12.0, 8.0) + ground_clearance_center(), 8.0);
    commands.spawn((
        Npc {
            id: 100,
            archetype: NpcArchetype::Knight,
        },
        NpcPosition(knight_pos),
        NpcRotation(0.0),
        Health::new(npc_max_health(NpcArchetype::Knight)),
        NpcWander::new(knight_pos, 8.0, 100), // Smaller wander radius for dialogue NPCs
        Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
    ));
    trace!("Spawned Knight NPC at {:?}", knight_pos);

    // Spawn RogueHooded NPC (dialogue NPC)
    let rogue_pos = Vec3::new(-5.0, terrain.get_height(-5.0, 12.0) + ground_clearance_center(), 12.0);
    commands.spawn((
        Npc {
            id: 101,
            archetype: NpcArchetype::RogueHooded,
        },
        NpcPosition(rogue_pos),
        NpcRotation(0.0),
        Health::new(npc_max_health(NpcArchetype::RogueHooded)),
        NpcWander::new(rogue_pos, 10.0, 101),
        Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
    ));
    trace!("Spawned RogueHooded NPC at {:?}", rogue_pos);
}

/// Spawn NPCs for the medieval town.
///
/// Distributes ~150 NPCs around the town:
/// - 2 Knights near the central manors
/// - ~110 RogueHooded throughout the town
/// - ~38 Barbarians as guards
pub fn spawn_medieval_town_npcs(
    commands: &mut Commands,
    terrain: &WorldTerrain,
    town_center: Vec3,
    npc_id_start: &mut u64,
) {
    use shared::structures::MEDIEVAL_SPACING;

    let mut rng = XorShift64::new(42_u64 ^ (town_center.x as u64) ^ (town_center.z as u64));

    // Spawn 2 Knights near the manors (east and west of town square)
    for (i, x_offset) in [18.0_f32, -18.0].iter().enumerate() {
        let npc_id = *npc_id_start;
        *npc_id_start += 1;

        let x = town_center.x + x_offset;
        let z = town_center.z + (rng.next_f32() - 0.5) * 8.0;
        let y = terrain.get_height(x, z) + ground_clearance_center();
        let pos = Vec3::new(x, y, z);

        commands.spawn((
            Npc {
                id: npc_id,
                archetype: NpcArchetype::Knight,
            },
            NpcPosition(pos),
            NpcRotation(if i == 0 { std::f32::consts::PI } else { 0.0 }), // Face center
            Health::new(npc_max_health(NpcArchetype::Knight)),
            NpcWander::new(pos, 12.0, npc_id), // Small wander radius for dialogue NPCs
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ));
        trace!("Spawned medieval town Knight {} at {:?}", npc_id, pos);
    }

    // Block offsets for distributing NPCs
    let block_offsets: [(i32, i32); 8] = [
        (-1, -1), (0, -1), (1, -1),
        (-1,  0),          (1,  0),
        (-1,  1), (0,  1), (1,  1),
    ];

    // Spawn RogueHooded (~110 total, ~12-14 per block)
    let rogues_per_block = 12;
    for (bx, bz) in block_offsets.iter() {
        let block_center_x = town_center.x + *bx as f32 * MEDIEVAL_SPACING;
        let block_center_z = town_center.z + *bz as f32 * MEDIEVAL_SPACING;

        for _ in 0..rogues_per_block {
            let npc_id = *npc_id_start;
            *npc_id_start += 1;

            // Random position within block area
            let offset_x = (rng.next_f32() - 0.5) * 35.0;
            let offset_z = (rng.next_f32() - 0.5) * 35.0;
            let x = block_center_x + offset_x;
            let z = block_center_z + offset_z;
            let y = terrain.get_height(x, z) + ground_clearance_center();
            let pos = Vec3::new(x, y, z);

            commands.spawn((
                Npc {
                    id: npc_id,
                    archetype: NpcArchetype::RogueHooded,
                },
                NpcPosition(pos),
                NpcRotation(rng.next_f32() * std::f32::consts::TAU),
                Health::new(npc_max_health(NpcArchetype::RogueHooded)),
                NpcWander::new(pos, 20.0, npc_id), // Medium wander radius
                Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
            ));
            trace!("Spawned medieval town RogueHooded {} at {:?}", npc_id, pos);
        }
    }

    // Add 14 more RogueHooded in the town square area
    for _ in 0..14 {
        let npc_id = *npc_id_start;
        *npc_id_start += 1;

        let x = town_center.x + (rng.next_f32() - 0.5) * 35.0;
        let z = town_center.z + (rng.next_f32() - 0.5) * 35.0;
        let y = terrain.get_height(x, z) + ground_clearance_center();
        let pos = Vec3::new(x, y, z);

        commands.spawn((
            Npc {
                id: npc_id,
                archetype: NpcArchetype::RogueHooded,
            },
            NpcPosition(pos),
            NpcRotation(rng.next_f32() * std::f32::consts::TAU),
            Health::new(npc_max_health(NpcArchetype::RogueHooded)),
            NpcWander::new(pos, 25.0, npc_id),
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ));
        trace!("Spawned medieval town square RogueHooded {} at {:?}", npc_id, pos);
    }

    // Spawn Barbarians as guards (~38 total, 3 per block + entrances)
    // Place them on the outer edges of blocks (near streets)
    let barbarians_per_block = 3;
    for (bx, bz) in block_offsets.iter() {
        let block_center_x = town_center.x + *bx as f32 * MEDIEVAL_SPACING;
        let block_center_z = town_center.z + *bz as f32 * MEDIEVAL_SPACING;

        for i in 0..barbarians_per_block {
            let npc_id = *npc_id_start;
            *npc_id_start += 1;

            // Position guards around the block perimeter
            let angle = (i as f32 / barbarians_per_block as f32) * std::f32::consts::TAU
                + rng.next_f32() * 0.5;
            let radius = 15.0 + rng.next_f32() * 8.0;
            let guard_x = block_center_x + angle.cos() * radius;
            let guard_z = block_center_z + angle.sin() * radius;
            let y = terrain.get_height(guard_x, guard_z) + ground_clearance_center();
            let pos = Vec3::new(guard_x, y, guard_z);

            commands.spawn((
                Npc {
                    id: npc_id,
                    archetype: NpcArchetype::Barbarian,
                },
                NpcPosition(pos),
                NpcRotation(rng.next_f32() * std::f32::consts::TAU),
                Health::new(npc_max_health(NpcArchetype::Barbarian)),
                NpcWander::new(pos, 15.0, npc_id),
                Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
            ));
            trace!("Spawned medieval town Barbarian guard {} at {:?}", npc_id, pos);
        }
    }

    // 14 more barbarians at key positions (town entrances and patrols)
    let entrance_positions = [
        (town_center.x, town_center.z - MEDIEVAL_SPACING * 1.3),  // South entrance
        (town_center.x, town_center.z + MEDIEVAL_SPACING * 1.3),  // North entrance
        (town_center.x - MEDIEVAL_SPACING * 1.3, town_center.z),  // West entrance
        (town_center.x + MEDIEVAL_SPACING * 1.3, town_center.z),  // East entrance
        (town_center.x, town_center.z),                            // Town square center
        // Additional patrol points
        (town_center.x + MEDIEVAL_SPACING * 0.7, town_center.z + MEDIEVAL_SPACING * 0.7),
        (town_center.x - MEDIEVAL_SPACING * 0.7, town_center.z + MEDIEVAL_SPACING * 0.7),
        (town_center.x + MEDIEVAL_SPACING * 0.7, town_center.z - MEDIEVAL_SPACING * 0.7),
        (town_center.x - MEDIEVAL_SPACING * 0.7, town_center.z - MEDIEVAL_SPACING * 0.7),
        (town_center.x + 10.0, town_center.z),  // Near east manor
        (town_center.x - 10.0, town_center.z),  // Near west manor
        (town_center.x, town_center.z + 15.0),  // North of square
        (town_center.x, town_center.z - 15.0),  // South of square
        (town_center.x + 5.0, town_center.z + 5.0),  // Square patrol
    ];

    for (ex, ez) in entrance_positions.iter() {
        let npc_id = *npc_id_start;
        *npc_id_start += 1;

        let x = ex + (rng.next_f32() - 0.5) * 6.0;
        let z = ez + (rng.next_f32() - 0.5) * 6.0;
        let y = terrain.get_height(x, z) + ground_clearance_center();
        let pos = Vec3::new(x, y, z);

        commands.spawn((
            Npc {
                id: npc_id,
                archetype: NpcArchetype::Barbarian,
            },
            NpcPosition(pos),
            NpcRotation(rng.next_f32() * std::f32::consts::TAU),
            Health::new(npc_max_health(NpcArchetype::Barbarian)),
            NpcWander::new(pos, 12.0, npc_id),
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ));
        trace!("Spawned medieval town entrance Barbarian {} at {:?}", npc_id, pos);
    }

    info!(
        "Spawned medieval town NPCs at center {:?}: 2 Knights, ~110 RogueHooded, ~38 Barbarians (~150 total)",
        town_center
    );
}

// =============================================================================
// AI / PATHFINDING
// =============================================================================

// NPC movement constants are now in shared/src/npc.rs:
// NPC_MOVE_SPEED, NPC_TURN_SPEED, NPC_WANDER_RADIUS, NPC_IDLE_TIME_MIN/MAX, NPC_MIN_TARGET_DIST

const GRID_CELL_SIZE: f32 = 2.0; // meters
const GRID_MAX_STEP: f32 = 1.2; // max height delta allowed between neighbor cells
const GRID_MAX_NODES: usize = 4000; // hard cap per path search (safety) - increased for larger paths

/// NPC AI state
pub enum NpcState {
    Idle,
    Walking,
    Fleeing {
        from_position: Vec3,
        flee_timer: f32,
        panic_speed_boost: f32,
    },
}

#[derive(Component)]
pub struct NpcWander {
    pub home: Vec3,
    pub target: Vec3,
    pub path: Vec<Vec3>,
    pub waypoint: usize,
    pub state: NpcState,
    /// When > 0, the NPC is idling. When it hits 0, pick a new target.
    pub idle_timer: f32,
    pub rng: XorShift64,
    // New fields for improved wandering
    pub current_speed_multiplier: f32,
    pub idle_rotation_target: f32,
    pub idle_rotation_speed: f32,
}

impl NpcWander {
    pub fn new(home: Vec3, _radius: f32, seed: u64) -> Self {
        let mut rng = XorShift64::new(seed ^ 0xC0FFEE_u64);
        // Start idling briefly so it doesn't immediately run off
        let idle_timer = 0.5 + rng.next_f32() * 1.0;
        Self {
            home,
            target: home,
            path: Vec::new(),
            waypoint: 0,
            state: NpcState::Idle,
            idle_timer,
            rng,
            current_speed_multiplier: 1.0,
            idle_rotation_target: 0.0,
            idle_rotation_speed: 0.3,
        }
    }
}

/// React to damage events by entering flee state
pub fn react_to_damage(
    mut commands: Commands,
    mut npcs: Query<(Entity, &mut NpcWander, &NpcPosition, &NpcDamageEvent)>,
) {
    for (entity, mut wander, _pos, damage_event) in npcs.iter_mut() {
        // Randomize flee parameters
        let flee_duration = 5.0 + wander.rng.next_f32() * 3.0; // 5-8 seconds
        let panic_boost = 1.5 + wander.rng.next_f32() * 0.3;   // 1.5-1.8x speed

        // Enter flee state (NPC will calculate flee direction in tick_fleeing_state)
        wander.state = NpcState::Fleeing {
            from_position: damage_event.damage_source_position,
            flee_timer: flee_duration,
            panic_speed_boost: panic_boost,
        };

        // Clear current path so flee logic takes over immediately
        wander.path.clear();
        wander.waypoint = 0;

        trace!(
            "NPC fleeing from damage source at {:?}, duration {:.1}s, speed boost {:.2}x",
            damage_event.damage_source_position,
            flee_duration,
            panic_boost
        );

        // Remove the damage event component (consumed)
        commands.entity(entity).remove::<NpcDamageEvent>();
    }
}

/// Tick wandering NPC AI (server-authoritative).
pub fn tick_npc_ai(
    terrain: Res<WorldTerrain>,
    obstacle_grid: Res<SpatialObstacleGrid>,
    mut npcs: Query<(&Npc, &mut NpcPosition, &mut NpcRotation, &Health, &mut NpcWander)>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    for (npc, mut pos, mut rot, health, mut wander) in npcs.iter_mut() {
        if health.is_dead() {
            // Stop movement when dead
            wander.path.clear();
            wander.waypoint = 0;
            continue;
        }

        match &wander.state {
            NpcState::Fleeing { from_position, flee_timer, panic_speed_boost } => {
                let from_pos = *from_position;
                let timer = *flee_timer;
                let boost = *panic_speed_boost;
                tick_fleeing_state(
                    &mut wander,
                    &mut pos,
                    &mut rot,
                    &terrain,
                    &obstacle_grid,
                    from_pos,
                    timer,
                    boost,
                    dt,
                    npc.id,
                );
            }
            NpcState::Idle => {
                tick_idle_state(&mut wander, &mut rot, &terrain, &obstacle_grid, pos.0, dt, npc.id);
            }
            NpcState::Walking => {
                tick_walking_state(&mut wander, &mut pos, &mut rot, &terrain, dt, npc.id);
            }
        }
    }
}

/// Tick idle state: rotate slowly, count down timer
fn tick_idle_state(
    wander: &mut NpcWander,
    rot: &mut NpcRotation,
    terrain: &WorldTerrain,
    obstacles: &SpatialObstacleGrid,
    current_pos: Vec3,
    dt: f32,
    npc_id: u64,
) {
    wander.idle_timer -= dt;

    // Slowly rotate toward idle target yaw
    let angle_diff = wander.idle_rotation_target - rot.0;
    let normalized_diff = ((angle_diff + std::f32::consts::PI) % (2.0 * std::f32::consts::PI))
        - std::f32::consts::PI;

    if normalized_diff.abs() > 0.01 {
        let turn_amount = wander.idle_rotation_speed * dt;
        rot.0 += normalized_diff.signum() * turn_amount.min(normalized_diff.abs());
    }

    if wander.idle_timer <= 0.0 {
        // Check if we were just pausing mid-path (still have waypoints to follow)
        let has_remaining_path = !wander.path.is_empty() && wander.waypoint < wander.path.len();

        if has_remaining_path {
            // Resume walking the current path
            wander.state = NpcState::Walking;
            trace!("NPC {} resuming walk after pause ({} waypoints remaining)", npc_id, wander.path.len() - wander.waypoint);
        } else {
            // Pick new wander target (avoiding obstacles)
            wander.target = pick_random_target(
                terrain,
                obstacles,
                wander.home,
                current_pos,
                NPC_WANDER_RADIUS,
                NPC_MIN_TARGET_DIST,
                &mut wander.rng,
            );
            wander.path = find_path_a_star(terrain, obstacles, current_pos, wander.target);
            wander.waypoint = 0;

            if wander.path.is_empty() {
                wander.path.push(wander.target); // Fallback
            }

            // Skip waypoints that are very close to current position (e.g., the start cell)
            while wander.waypoint < wander.path.len() {
                let wp = wander.path[wander.waypoint];
                let dist = Vec2::new(wp.x - current_pos.x, wp.z - current_pos.z).length();
                if dist < 1.0 {
                    wander.waypoint += 1;
                } else {
                    break;
                }
            }

            // Randomize walk speed for this session
            wander.current_speed_multiplier = 0.9 + wander.rng.next_f32() * 0.2; // 0.9-1.1

            wander.state = NpcState::Walking;

            trace!(
                "NPC {} walking to {:?} (distance: {:.1}m, path {} waypoints, speed {:.2}x)",
                npc_id,
                wander.target,
                (wander.target - current_pos).length(),
                wander.path.len(),
                wander.current_speed_multiplier
            );
        }
    }
}

/// Smoothly rotate current angle toward target angle at a given speed.
/// Returns the new angle after rotation.
fn smooth_rotate_toward(current: f32, target: f32, turn_speed: f32, dt: f32) -> f32 {
    use std::f32::consts::PI;

    // Normalize angle difference to [-PI, PI]
    let mut diff = target - current;
    while diff > PI {
        diff -= 2.0 * PI;
    }
    while diff < -PI {
        diff += 2.0 * PI;
    }

    // Turn at most turn_speed * dt radians this frame
    let max_turn = turn_speed * dt;
    if diff.abs() <= max_turn {
        target
    } else {
        current + diff.signum() * max_turn
    }
}

/// Tick walking state: follow path, handle waypoints, add variety
fn tick_walking_state(
    wander: &mut NpcWander,
    pos: &mut NpcPosition,
    rot: &mut NpcRotation,
    terrain: &WorldTerrain,
    dt: f32,
    npc_id: u64,
) {
    let path_complete = wander.path.is_empty() || wander.waypoint >= wander.path.len();

    if path_complete {
        // Start idling
        wander.idle_timer = NPC_IDLE_TIME_MIN + wander.rng.next_f32() * (NPC_IDLE_TIME_MAX - NPC_IDLE_TIME_MIN);

        // Pick random rotation target for idle look-around
        wander.idle_rotation_target = wander.rng.next_f32() * std::f32::consts::TAU;

        wander.state = NpcState::Idle;
        trace!("NPC {} reached destination, idling for {:.1}s", npc_id, wander.idle_timer);
        return;
    }

    let Some(waypoint) = wander.path.get(wander.waypoint).copied() else {
        return;
    };

    let to = waypoint - pos.0;
    let dist_xz = Vec2::new(to.x, to.z).length();

    if dist_xz < 0.6 {
        // Reached waypoint
        wander.waypoint += 1;

        // Check if completed entire path
        if wander.waypoint >= wander.path.len() {
            wander.idle_timer = NPC_IDLE_TIME_MIN + wander.rng.next_f32() * (NPC_IDLE_TIME_MAX - NPC_IDLE_TIME_MIN);
            wander.idle_rotation_target = wander.rng.next_f32() * std::f32::consts::TAU;
            wander.state = NpcState::Idle;
            trace!("NPC {} reached destination, idling for {:.1}s", npc_id, wander.idle_timer);
            return;
        }

        // Add variety: 20% chance to pause briefly at waypoint
        if wander.rng.next_f32() < 0.2 {
            wander.idle_timer = 0.5 + wander.rng.next_f32() * 0.5; // 0.5-1s pause
            wander.idle_rotation_target = rot.0; // Don't turn, just pause
            wander.state = NpcState::Idle;
            trace!("NPC {} pausing at waypoint for {:.1}s", npc_id, wander.idle_timer);
            return;
        }

        return;
    }

    // Calculate target facing direction
    let dir_xz = Vec2::new(to.x, to.z).normalize_or_zero();
    let target_yaw = (-dir_xz.x).atan2(-dir_xz.y);

    // Smoothly rotate toward movement direction
    rot.0 = smooth_rotate_toward(rot.0, target_yaw, NPC_TURN_SPEED, dt);

    // Move toward waypoint (in the direction we're facing, not the target direction)
    // This makes NPCs turn before walking in a new direction, which looks more natural
    let facing_dir = Vec2::new(-rot.0.sin(), -rot.0.cos());
    let speed = NPC_MOVE_SPEED * wander.current_speed_multiplier;

    // Only move at full speed if facing roughly the right direction (within ~60 degrees)
    let alignment = dir_xz.dot(facing_dir);
    let move_factor = alignment.max(0.0); // Don't walk backwards

    let step = Vec3::new(facing_dir.x, 0.0, facing_dir.y) * (speed * move_factor * dt);

    pos.0.x += step.x;
    pos.0.z += step.z;

    // Stay glued to heightfield
    let ground_y = terrain.get_height(pos.0.x, pos.0.z);
    pos.0.y = ground_y + ground_clearance_center();
}

/// Tick fleeing state: run away from threat, decrease timer
fn tick_fleeing_state(
    wander: &mut NpcWander,
    pos: &mut NpcPosition,
    rot: &mut NpcRotation,
    terrain: &WorldTerrain,
    obstacles: &SpatialObstacleGrid,
    from_position: Vec3,
    mut flee_timer: f32,
    panic_speed_boost: f32,
    dt: f32,
    npc_id: u64,
) {
    flee_timer -= dt;

    // Check if flee duration expired
    if flee_timer <= 0.0 {
        wander.idle_timer = NPC_IDLE_TIME_MIN + wander.rng.next_f32() * (NPC_IDLE_TIME_MAX - NPC_IDLE_TIME_MIN);
        wander.idle_rotation_target = wander.rng.next_f32() * std::f32::consts::TAU;
        wander.state = NpcState::Idle;
        wander.path.clear();
        wander.waypoint = 0;
        trace!("NPC {} stopped fleeing, returning to idle", npc_id);
        return;
    }

    // If no flee path, generate one
    if wander.path.is_empty() || wander.waypoint >= wander.path.len() {
        let away_vec = pos.0 - from_position;
        let away_dir = Vec2::new(away_vec.x, away_vec.z).normalize_or_zero();

        // Pick flee target 20-30m away in opposite direction
        let flee_distance = 20.0 + wander.rng.next_f32() * 10.0;
        let flee_target_xz = Vec2::new(pos.0.x, pos.0.z) + away_dir * flee_distance;
        let flee_y = terrain.get_height(flee_target_xz.x, flee_target_xz.y);
        let flee_target = Vec3::new(flee_target_xz.x, flee_y + ground_clearance_center(), flee_target_xz.y);

        // Try to pathfind to flee target (avoiding obstacles)
        wander.path = find_path_a_star(terrain, obstacles, pos.0, flee_target);
        wander.waypoint = 0;

        if wander.path.is_empty() {
            // Pathfinding failed - just run in a random direction
            let random_angle = wander.rng.next_f32() * std::f32::consts::TAU;
            let random_dir = Vec2::new(random_angle.cos(), random_angle.sin());
            let fallback_target_xz = Vec2::new(pos.0.x, pos.0.z) + random_dir * 15.0;
            let fallback_y = terrain.get_height(fallback_target_xz.x, fallback_target_xz.y);
            wander.path.push(Vec3::new(fallback_target_xz.x, fallback_y + ground_clearance_center(), fallback_target_xz.y));
        }

        // Skip waypoints that are very close to current position (e.g., the start cell)
        while wander.waypoint < wander.path.len() {
            let wp = wander.path[wander.waypoint];
            let dist = Vec2::new(wp.x - pos.0.x, wp.z - pos.0.z).length();
            if dist < 1.0 {
                wander.waypoint += 1;
            } else {
                break;
            }
        }

        trace!("NPC {} fleeing toward {:?}, timer {:.1}s remaining", npc_id, flee_target, flee_timer);
    }

    // Follow flee path
    let Some(waypoint) = wander.path.get(wander.waypoint).copied() else {
        return;
    };

    let to = waypoint - pos.0;
    let dist_xz = Vec2::new(to.x, to.z).length();

    if dist_xz < 0.6 {
        wander.waypoint += 1;
        // Don't stop fleeing even if we reach the target - keep timer running
        return;
    }

    // Calculate target facing direction
    let dir_xz = Vec2::new(to.x, to.z).normalize_or_zero();
    let target_yaw = (-dir_xz.x).atan2(-dir_xz.y);

    // Smoothly rotate toward flee direction (faster turn speed when panicking)
    let panic_turn_speed = NPC_TURN_SPEED * 1.5;
    rot.0 = smooth_rotate_toward(rot.0, target_yaw, panic_turn_speed, dt);

    // Move in the direction we're facing
    let facing_dir = Vec2::new(-rot.0.sin(), -rot.0.cos());
    let flee_speed = NPC_MOVE_SPEED * panic_speed_boost;

    // Only move at full speed if facing roughly the right direction
    let alignment = dir_xz.dot(facing_dir);
    let move_factor = alignment.max(0.0);

    let step = Vec3::new(facing_dir.x, 0.0, facing_dir.y) * (flee_speed * move_factor * dt);

    pos.0.x += step.x;
    pos.0.z += step.z;

    let ground_y = terrain.get_height(pos.0.x, pos.0.z);
    pos.0.y = ground_y + ground_clearance_center();

    // Update flee timer in state
    wander.state = NpcState::Fleeing {
        from_position,
        flee_timer,
        panic_speed_boost,
    };
}

// =============================================================================
// DEAD NPC CLEANUP
// =============================================================================

/// Add despawn timer to newly dead NPCs
pub fn add_despawn_timer_to_dead_npcs(
    mut commands: Commands,
    dead_npcs: Query<(Entity, &Health), (With<Npc>, Without<DeadNpcDespawnTimer>)>,
) {
    for (entity, health) in dead_npcs.iter() {
        if health.is_dead() {
            commands.entity(entity).insert(DeadNpcDespawnTimer(DEAD_NPC_DESPAWN_TIME));
            trace!("Added despawn timer to dead NPC {:?}", entity);
        }
    }
}

/// Tick down despawn timers and remove NPCs that have been dead long enough
pub fn tick_dead_npc_despawn_timers(
    mut commands: Commands,
    mut dead_npcs: Query<(Entity, &Npc, &mut DeadNpcDespawnTimer)>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    for (entity, npc, mut timer) in dead_npcs.iter_mut() {
        timer.0 -= dt;

        if timer.0 <= 0.0 {
            info!("Despawning dead NPC {} ({:?}) after timeout", npc.id, npc.archetype);
            commands.entity(entity).despawn();
        }
    }
}

// =============================================================================
// WANDER TARGET SELECTION
// =============================================================================

fn pick_random_target(
    terrain: &WorldTerrain,
    obstacles: &SpatialObstacleGrid,
    home: Vec3,
    current_pos: Vec3,
    max_radius: f32,
    min_dist: f32,
    rng: &mut XorShift64,
) -> Vec3 {
    // Try multiple times to pick a target that's:
    // - At least min_dist away from current position
    // - Not inside any obstacle footprint
    for _ in 0..16 {
        let angle = rng.next_f32() * std::f32::consts::TAU;
        // Bias toward further distances (use sqrt for uniform disk, skip for outer ring bias)
        let r = min_dist + (max_radius - min_dist) * rng.next_f32();
        let x = home.x + angle.cos() * r;
        let z = home.z + angle.sin() * r;
        let y = terrain.get_height(x, z) + ground_clearance_center();
        let candidate = Vec3::new(x, y, z);

        // Check if candidate is inside any obstacle footprint (O(1) spatial lookup)
        let candidate_xz = Vec2::new(candidate.x, candidate.z);
        if obstacles.point_blocked(candidate_xz) {
            continue; // Skip - target is inside an obstacle
        }

        let dist_from_current = Vec2::new(candidate.x - current_pos.x, candidate.z - current_pos.z).length();
        if dist_from_current >= min_dist {
            return candidate;
        }
    }

    // Fallback: just pick something in the outer ring (even if inside obstacle - pathfinding will avoid)
    let angle = rng.next_f32() * std::f32::consts::TAU;
    let r = max_radius * 0.7 + max_radius * 0.3 * rng.next_f32();
    let x = home.x + angle.cos() * r;
    let z = home.z + angle.sin() * r;
    let y = terrain.get_height(x, z) + ground_clearance_center();
    Vec3::new(x, y, z)
}

// =============================================================================
// PATHFINDING (grid A*)
// =============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GridPos {
    x: i32,
    z: i32,
}

fn world_to_grid(p: Vec3) -> GridPos {
    GridPos {
        x: (p.x / GRID_CELL_SIZE).round() as i32,
        z: (p.z / GRID_CELL_SIZE).round() as i32,
    }
}

fn grid_to_world(terrain: &WorldTerrain, g: GridPos) -> Vec3 {
    let x = g.x as f32 * GRID_CELL_SIZE;
    let z = g.z as f32 * GRID_CELL_SIZE;
    let y = terrain.get_height(x, z) + ground_clearance_center();
    Vec3::new(x, y, z)
}

fn heuristic(a: GridPos, b: GridPos) -> f32 {
    // Euclidean in grid-space
    let dx = (a.x - b.x) as f32;
    let dz = (a.z - b.z) as f32;
    (dx * dx + dz * dz).sqrt()
}

fn find_path_a_star(
    terrain: &WorldTerrain,
    obstacles: &SpatialObstacleGrid,
    start_world: Vec3,
    goal_world: Vec3,
) -> Vec<Vec3> {
    use std::cmp::Ordering;
    use std::collections::{BinaryHeap, HashMap};

    let start = world_to_grid(start_world);
    let goal = world_to_grid(goal_world);

    if start == goal {
        return vec![goal_world];
    }

    #[derive(Clone, Copy, Debug)]
    struct OpenNode {
        f_cost: i32,
        pos: GridPos,
    }

    impl Eq for OpenNode {}
    impl PartialEq for OpenNode {
        fn eq(&self, other: &Self) -> bool {
            self.f_cost == other.f_cost && self.pos == other.pos
        }
    }
    impl Ord for OpenNode {
        fn cmp(&self, other: &Self) -> Ordering {
            // Reverse for min-heap behavior.
            other
                .f_cost
                .cmp(&self.f_cost)
                .then_with(|| self.pos.x.cmp(&other.pos.x))
                .then_with(|| self.pos.z.cmp(&other.pos.z))
        }
    }
    impl PartialOrd for OpenNode {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    let mut open = BinaryHeap::new();
    let mut came_from: HashMap<GridPos, GridPos> = HashMap::new();
    let mut g_score: HashMap<GridPos, f32> = HashMap::new();
    let mut height_cache: HashMap<GridPos, f32> = HashMap::new();

    let height = |p: GridPos, terrain: &WorldTerrain, cache: &mut HashMap<GridPos, f32>| -> f32 {
        if let Some(h) = cache.get(&p) {
            return *h;
        }
        let w = grid_to_world(terrain, p);
        let h = w.y;
        cache.insert(p, h);
        h
    };

    g_score.insert(start, 0.0);
    open.push(OpenNode {
        f_cost: (heuristic(start, goal) * 1000.0) as i32,
        pos: start,
    });

    let neighbors = |p: GridPos| -> [GridPos; 8] {
        [
            GridPos { x: p.x + 1, z: p.z },
            GridPos { x: p.x - 1, z: p.z },
            GridPos { x: p.x, z: p.z + 1 },
            GridPos { x: p.x, z: p.z - 1 },
            GridPos { x: p.x + 1, z: p.z + 1 },
            GridPos { x: p.x + 1, z: p.z - 1 },
            GridPos { x: p.x - 1, z: p.z + 1 },
            GridPos { x: p.x - 1, z: p.z - 1 },
        ]
    };

    let mut expanded = 0_usize;
    while let Some(OpenNode { pos: current, .. }) = open.pop() {
        expanded += 1;
        if expanded > GRID_MAX_NODES {
            return Vec::new();
        }

        if current == goal {
            // Reconstruct
            let mut path = vec![current];
            let mut cur = current;
            while let Some(prev) = came_from.get(&cur).copied() {
                path.push(prev);
                cur = prev;
            }
            path.reverse();
            return path.into_iter().map(|gp| grid_to_world(terrain, gp)).collect();
        }

        let current_h = height(current, terrain, &mut height_cache);

        for n in neighbors(current) {
            let n_h = height(n, terrain, &mut height_cache);
            if (n_h - current_h).abs() > GRID_MAX_STEP {
                continue; // too steep / cliff
            }

            // Check if neighbor is inside any obstacle footprint (O(1) spatial lookup)
            let neighbor_world = grid_to_world(terrain, n);
            let neighbor_xz = Vec2::new(neighbor_world.x, neighbor_world.z);
            if obstacles.point_blocked(neighbor_xz) {
                continue; // Skip - inside an obstacle
            }

            let diag = (n.x != current.x) && (n.z != current.z);
            let step_cost = if diag { 1.4142 } else { 1.0 };

            let tentative_g = g_score.get(&current).copied().unwrap_or(f32::INFINITY) + step_cost;
            if tentative_g < g_score.get(&n).copied().unwrap_or(f32::INFINITY) {
                came_from.insert(n, current);
                g_score.insert(n, tentative_g);

                let f = tentative_g + heuristic(n, goal);
                open.push(OpenNode {
                    f_cost: (f * 1000.0) as i32,
                    pos: n,
                });
            }
        }
    }

    Vec::new()
}

// =============================================================================
// SMALL DETERMINISTIC RNG
// =============================================================================

/// Tiny deterministic RNG (fast, no external deps).
#[derive(Clone, Copy, Debug)]
pub struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    pub fn next_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    pub fn next_f32(&mut self) -> f32 {
        // Use 24 bits of mantissa precision (matches f32 mantissa size)
        let v = (self.next_u64() >> 40) as u32;
        (v as f32) / ((1u32 << 24) as f32)  // Divide by 2^24 to get [0, 1) range
    }
}

// Silence unused warnings until we hook these helpers into bullet hit detection / debug.
#[allow(dead_code)]
fn _hitbox_helpers_smoke_test(p: Vec3) {
    let _ = npc_head_center(p);
    let _ = npc_capsule_endpoints(p);
}

