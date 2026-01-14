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
    NpcRotation, WorldTerrain, FIXED_TIMESTEP_HZ, Health,
};

// =============================================================================
// SPAWN
// =============================================================================

/// One-shot resource to ensure NPCs are only spawned once.
#[derive(Resource)]
pub struct NpcsSpawned;

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

    // Spawn 3 NPCs at different positions around the player spawn
    let spawn_positions = [
        (6.0, -4.0),   // Original position
        (-8.0, -6.0),  // Left side
        (4.0, 10.0),   // Behind/south
    ];

    for (npc_id, (x, z)) in spawn_positions.iter().enumerate() {
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
            Health::new(120.0),
            NpcWander::new(pos, 18.0, npc_id),
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ));

        trace!("Spawned NPC {} near spawn at {:?}", npc_id, pos);
    }
}

// =============================================================================
// AI / PATHFINDING
// =============================================================================

const NPC_MOVE_SPEED: f32 = 3.5; // m/s
const NPC_IDLE_TIME_MIN: f32 = 1.5; // seconds to idle after reaching a target
const NPC_IDLE_TIME_MAX: f32 = 4.0;
const NPC_MIN_TARGET_DIST: f32 = 15.0; // minimum distance for a new wander target
const NPC_WANDER_RADIUS: f32 = 60.0; // how far NPCs can wander from home

const GRID_CELL_SIZE: f32 = 2.0; // meters
const GRID_MAX_STEP: f32 = 1.2; // max height delta allowed between neighbor cells
const GRID_MAX_NODES: usize = 4000; // hard cap per path search (safety) - increased for larger paths

#[derive(Component)]
pub struct NpcWander {
    pub home: Vec3,
    pub target: Vec3,
    pub path: Vec<Vec3>,
    pub waypoint: usize,
    /// When > 0, the NPC is idling. When it hits 0, pick a new target.
    pub idle_timer: f32,
    pub rng: XorShift64,
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
            idle_timer,
            rng,
        }
    }
}

/// Tick wandering NPC AI (server-authoritative).
pub fn tick_npc_ai(
    terrain: Res<WorldTerrain>,
    mut npcs: Query<(&Npc, &mut NpcPosition, &mut NpcRotation, &Health, &mut NpcWander)>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    for (npc, mut pos, mut rot, health, mut wander) in npcs.iter_mut() {
        if health.is_dead() {
            // Stop movement when dead.
            wander.path.clear();
            wander.waypoint = 0;
            continue;
        }

        // Check if we've reached our current path destination
        let path_complete = wander.path.is_empty() || wander.waypoint >= wander.path.len();

        if path_complete {
            // We finished our path (or don't have one), so idle before picking a new target
            wander.idle_timer -= dt;

            if wander.idle_timer <= 0.0 {
                // Pick a new target that's reasonably far away
                wander.target = pick_random_target(
                    &terrain,
                    wander.home,
                    pos.0,
                    NPC_WANDER_RADIUS,
                    NPC_MIN_TARGET_DIST,
                    &mut wander.rng,
                );
                wander.path = find_path_a_star(&terrain, pos.0, wander.target);
                wander.waypoint = 0;

                // Fallback: if pathfinding fails, just walk straight toward target.
                if wander.path.is_empty() {
                    let target = wander.target;
                    wander.path.push(target);
                }

                trace!(
                    "NPC {} walking to {:?} (distance: {:.1}m, path {} waypoints)",
                    npc.id,
                    wander.target,
                    (wander.target - pos.0).length(),
                    wander.path.len()
                );
            }
            continue; // Don't move while idling
        }

        // Follow current waypoint
        let Some(waypoint) = wander.path.get(wander.waypoint).copied() else {
            continue;
        };

        let to = waypoint - pos.0;
        let dist_xz = Vec2::new(to.x, to.z).length();
        if dist_xz < 0.6 {
            wander.waypoint += 1;
            // Check if we just completed the entire path
            if wander.waypoint >= wander.path.len() {
                // Start idling
                wander.idle_timer =
                    NPC_IDLE_TIME_MIN + wander.rng.next_f32() * (NPC_IDLE_TIME_MAX - NPC_IDLE_TIME_MIN);
                trace!("NPC {} reached destination, idling for {:.1}s", npc.id, wander.idle_timer);
            }
            continue;
        }

        let dir_xz = Vec2::new(to.x, to.z).normalize_or_zero();
        let step = Vec3::new(dir_xz.x, 0.0, dir_xz.y) * (NPC_MOVE_SPEED * dt);

        pos.0.x += step.x;
        pos.0.z += step.z;

        // Stay glued to the deterministic heightfield.
        let ground_y = terrain.get_height(pos.0.x, pos.0.z);
        pos.0.y = ground_y + ground_clearance_center();

        // Face movement direction.
        rot.0 = (-dir_xz.x).atan2(-dir_xz.y);
    }
}

fn pick_random_target(
    terrain: &WorldTerrain,
    home: Vec3,
    current_pos: Vec3,
    max_radius: f32,
    min_dist: f32,
    rng: &mut XorShift64,
) -> Vec3 {
    // Try a few times to pick a target that's at least min_dist away from current position
    for _ in 0..8 {
        let angle = rng.next_f32() * std::f32::consts::TAU;
        // Bias toward further distances (use sqrt for uniform disk, skip for outer ring bias)
        let r = min_dist + (max_radius - min_dist) * rng.next_f32();
        let x = home.x + angle.cos() * r;
        let z = home.z + angle.sin() * r;
        let y = terrain.get_height(x, z) + ground_clearance_center();
        let candidate = Vec3::new(x, y, z);

        let dist_from_current = Vec2::new(candidate.x - current_pos.x, candidate.z - current_pos.z).length();
        if dist_from_current >= min_dist {
            return candidate;
        }
    }

    // Fallback: just pick something in the outer ring
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

fn find_path_a_star(terrain: &WorldTerrain, start_world: Vec3, goal_world: Vec3) -> Vec<Vec3> {
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
        // 24 bits of mantissa precision is fine.
        let v = (self.next_u64() >> 40) as u32;
        (v as f32) / (u32::MAX as f32)
    }
}

// Silence unused warnings until we hook these helpers into bullet hit detection / debug.
#[allow(dead_code)]
fn _hitbox_helpers_smoke_test(p: Vec3) {
    let _ = npc_head_center(p);
    let _ = npc_capsule_endpoints(p);
}

