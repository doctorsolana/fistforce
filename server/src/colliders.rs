//! Server-side static collider streaming for world props.
//!
//! Loads baked collider shapes from `client/assets/colliders.bin` and maintains a
//! chunked + spatial-hashed set of static colliders near active players.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use shared::{
    npc::{NPC_HEIGHT, NPC_RADIUS},
    player::{PLAYER_HEIGHT, PLAYER_RADIUS, STEP_UP_HEIGHT},
    vehicle::motorbike,
    Health, InVehicle, Npc, NpcPosition, Player, PlayerPosition, PlayerVelocity, PropKind,
    VehicleState, WorldTerrain, ChunkCoord,
    DesertStructureKind, StructureCollider, generate_chunk_structures,
};

/// How many chunks around each player we keep static colliders loaded for.
///
/// Collisions are only needed near players/NPCs/vehicles.
const COLLIDER_VIEW_DISTANCE_CHUNKS: i32 = 3;

/// Limit how many chunks we load per fixed tick.
///
/// Without this, a newly joined player (especially if other players are far away) can cause
/// a large number of collider chunks to be generated and indexed in a single tick, which can
/// stall the server and trigger client netcode timeouts ("stuck until reconnect").
const MAX_COLLIDER_CHUNKS_TO_LOAD_PER_TICK: usize = 6;

/// Spatial hash cell size in meters.
const COLLIDER_CELL_SIZE: f32 = 16.0;

fn cell_key(x: f32, z: f32) -> (i32, i32) {
    ((x / COLLIDER_CELL_SIZE).floor() as i32, (z / COLLIDER_CELL_SIZE).floor() as i32)
}

/// A baked collider library keyed by [`PropKind`].
#[derive(Resource)]
pub struct BakedColliderLibrary {
    pub by_kind: HashMap<PropKind, shared::BakedCollider>,
}

/// Derived collision info from the baked hull points.
///
/// Uses actual 3D convex hull for accurate collision.
#[derive(Resource)]
pub struct DerivedColliderLibrary {
    pub by_kind: HashMap<PropKind, DerivedCollider>,
}

/// A face of the convex hull (triangle).
#[derive(Clone, Debug)]
pub struct HullFace {
    pub vertices: [Vec3; 3],
    pub normal: Vec3,
    pub d: f32, // plane equation: normal.dot(p) = d
}

#[derive(Clone, Debug)]
pub struct DerivedCollider {
    /// Bounding radius for broad-phase rejection
    pub bounding_radius: f32,
    /// Triangulated faces of the convex hull with outward normals
    pub hull_faces: Vec<HullFace>,
}

/// A single static collider instance in the world (one prop spawn).
#[derive(Clone, Debug)]
pub struct StaticColliderInstance {
    pub kind: PropKind,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: f32,
    pub cell: (i32, i32),
}

/// Streaming state for static colliders.
#[derive(Resource, Default)]
pub struct StaticColliders {
    pub loaded_chunks: HashSet<ChunkCoord>,
    /// Chunk -> list of instance ids
    pub chunk_instances: HashMap<ChunkCoord, Vec<u32>>,
    /// Instance id -> instance
    pub instances: HashMap<u32, StaticColliderInstance>,
    /// Spatial hash cell -> instance ids
    pub cells: HashMap<(i32, i32), Vec<u32>>,
    pub next_id: u32,
}

/// A structure collider instance
#[derive(Clone, Debug)]
pub struct StructureColliderInstance {
    pub kind: DesertStructureKind,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: f32,
    pub cell: (i32, i32),
}

/// Streaming state for structure colliders.
#[derive(Resource, Default)]
pub struct StructureColliders {
    pub loaded_chunks: HashSet<ChunkCoord>,
    pub chunk_instances: HashMap<ChunkCoord, Vec<u32>>,
    pub instances: HashMap<u32, StructureColliderInstance>,
    pub cells: HashMap<(i32, i32), Vec<u32>>,
    pub next_id: u32,
}

/// Load baked colliders at startup.
///
/// For now we load from the workspace path `client/assets/colliders.bin`.
pub fn load_baked_colliders(mut commands: Commands) {
    let path = "client/assets/colliders.bin";
    let db = shared::load_baked_collider_db_from_file(path)
        .unwrap_or_else(|e| panic!("Failed to load baked colliders from {path}: {e}"));

    let mut by_kind = HashMap::new();
    let mut derived = HashMap::new();
    for kind in shared::ALL_PROP_KINDS.iter().copied() {
        if let Some(c) = db.entries.get(kind.id()).cloned() {
            if let Some(d) = derive_collider(&c) {
                derived.insert(kind, d);
                by_kind.insert(kind, c);
            } else {
                warn!("Baked collider for {} has no usable points; skipping", kind.id());
            }
        }
    }

    info!(
        "Loaded baked colliders: {} entries (db version {})",
        by_kind.len(),
        db.version
    );

    commands.insert_resource(BakedColliderLibrary { by_kind });
    commands.insert_resource(DerivedColliderLibrary { by_kind: derived });
    commands.init_resource::<StaticColliders>();
    commands.init_resource::<StructureColliders>();
}

/// Stream in/out static colliders based on player positions.
///
/// This only manages the index; actual collision resolution is handled elsewhere.
pub fn stream_static_colliders(
    terrain: Res<WorldTerrain>,
    library: Option<Res<BakedColliderLibrary>>,
    players: Query<&PlayerPosition>,
    mut colliders: ResMut<StaticColliders>,
) {
    let Some(library) = library else { return };

    // Compute desired chunks (union around all players).
    let mut desired: HashSet<ChunkCoord> = HashSet::new();
    let mut player_centers: Vec<ChunkCoord> = Vec::new();
    for pos in players.iter() {
        let center = ChunkCoord::from_world_pos(pos.0);
        desired.extend(center.chunks_in_radius(COLLIDER_VIEW_DISTANCE_CHUNKS));
        player_centers.push(center);
    }

    // Unload chunks that are no longer desired.
    let to_unload: Vec<ChunkCoord> = colliders
        .loaded_chunks
        .difference(&desired)
        .copied()
        .collect();
    for chunk in to_unload {
        unload_chunk(&mut colliders, chunk);
    }

    // Load new chunks.
    let mut to_load: Vec<ChunkCoord> = desired
        .difference(&colliders.loaded_chunks)
        .copied()
        .collect();

    // Sort by nearest player chunk (Chebyshev distance) so we load what matters first.
    // This also stabilizes perf spikes on joins.
    to_load.sort_by_key(|c| {
        player_centers
            .iter()
            .map(|p| (c.x - p.x).abs().max((c.z - p.z).abs()))
            .min()
            .unwrap_or(i32::MAX)
    });

    for chunk in to_load.into_iter().take(MAX_COLLIDER_CHUNKS_TO_LOAD_PER_TICK) {
        load_chunk(&terrain, &library, &mut colliders, chunk);
    }
}

/// Stream in/out structure colliders based on player positions.
pub fn stream_structure_colliders(
    terrain: Res<WorldTerrain>,
    players: Query<&PlayerPosition>,
    mut colliders: ResMut<StructureColliders>,
) {
    // Compute desired chunks (union around all players).
    let mut desired: HashSet<ChunkCoord> = HashSet::new();
    let mut player_centers: Vec<ChunkCoord> = Vec::new();
    for pos in players.iter() {
        let center = ChunkCoord::from_world_pos(pos.0);
        desired.extend(center.chunks_in_radius(COLLIDER_VIEW_DISTANCE_CHUNKS));
        player_centers.push(center);
    }

    // Unload chunks that are no longer desired.
    let to_unload: Vec<ChunkCoord> = colliders
        .loaded_chunks
        .difference(&desired)
        .copied()
        .collect();
    for chunk in to_unload {
        unload_structure_chunk(&mut colliders, chunk);
    }

    // Load new chunks.
    let mut to_load: Vec<ChunkCoord> = desired
        .difference(&colliders.loaded_chunks)
        .copied()
        .collect();

    to_load.sort_by_key(|c| {
        player_centers
            .iter()
            .map(|p| (c.x - p.x).abs().max((c.z - p.z).abs()))
            .min()
            .unwrap_or(i32::MAX)
    });

    for chunk in to_load.into_iter().take(MAX_COLLIDER_CHUNKS_TO_LOAD_PER_TICK) {
        load_structure_chunk(&terrain, &mut colliders, chunk);
    }
}

fn unload_structure_chunk(colliders: &mut StructureColliders, chunk: ChunkCoord) {
    colliders.loaded_chunks.remove(&chunk);

    let Some(ids) = colliders.chunk_instances.remove(&chunk) else { return };
    for id in ids {
        if let Some(inst) = colliders.instances.remove(&id) {
            if let Some(cell_list) = colliders.cells.get_mut(&inst.cell) {
                cell_list.retain(|x| *x != id);
                if cell_list.is_empty() {
                    colliders.cells.remove(&inst.cell);
                }
            }
        }
    }
}

fn load_structure_chunk(
    terrain: &WorldTerrain,
    colliders: &mut StructureColliders,
    chunk: ChunkCoord,
) {
    let spawns = generate_chunk_structures(&terrain.generator, chunk);

    let mut ids = Vec::new();
    for spawn in spawns {
        let id = colliders.next_id;
        colliders.next_id = colliders.next_id.wrapping_add(1);

        let cell = cell_key(spawn.position.x, spawn.position.z);
        let inst = StructureColliderInstance {
            kind: spawn.kind,
            position: spawn.position,
            rotation: spawn.rotation,
            scale: spawn.scale,
            cell,
        };

        colliders.instances.insert(id, inst);
        colliders.cells.entry(cell).or_default().push(id);
        ids.push(id);
    }

    colliders.loaded_chunks.insert(chunk);
    colliders.chunk_instances.insert(chunk, ids);
}

/// Resolve player collisions against static colliders (server-authoritative).
pub fn resolve_player_static_collisions(
    terrain: Res<WorldTerrain>,
    derived: Option<Res<DerivedColliderLibrary>>,
    colliders: Res<StaticColliders>,
    structure_colliders: Res<StructureColliders>,
    mut players: Query<(&mut PlayerPosition, &mut PlayerVelocity, Option<&InVehicle>), With<Player>>,
) {
    let Some(derived) = derived else { return };

    for (mut pos, mut vel, in_vehicle) in players.iter_mut() {
        if in_vehicle.is_some() {
            continue;
        }

        // Resolve against props
        resolve_capsule_vs_static(
            &derived,
            &colliders,
            &mut pos.0,
            Some(&mut vel.0),
            PLAYER_RADIUS,
            PLAYER_HEIGHT,
            STEP_UP_HEIGHT,
        );

        // Resolve against structures
        resolve_capsule_vs_structures(
            &structure_colliders,
            &mut pos.0,
            Some(&mut vel.0),
            PLAYER_RADIUS,
            PLAYER_HEIGHT,
        );

        // Only snap UP if player fell below terrain (don't snap down — that kills jumping).
        let ground_y = terrain.generator.get_height(pos.0.x, pos.0.z);
        let min_y = ground_y + shared::ground_clearance_center();
        if pos.0.y < min_y {
            pos.0.y = min_y;
            // Kill downward velocity on ground snap
            if vel.0.y < 0.0 {
                vel.0.y = 0.0;
            }
        }
    }
}

/// Resolve NPC collisions against static colliders (server-authoritative).
pub fn resolve_npc_static_collisions(
    terrain: Res<WorldTerrain>,
    derived: Option<Res<DerivedColliderLibrary>>,
    colliders: Res<StaticColliders>,
    structure_colliders: Res<StructureColliders>,
    mut npcs: Query<(&mut NpcPosition, &Health), With<Npc>>,
) {
    let Some(derived) = derived else { return };

    for (mut pos, health) in npcs.iter_mut() {
        if health.is_dead() {
            continue;
        }

        // Resolve against props
        resolve_capsule_vs_static(
            &derived,
            &colliders,
            &mut pos.0,
            None,
            NPC_RADIUS,
            NPC_HEIGHT,
            STEP_UP_HEIGHT,
        );

        // Resolve against structures
        resolve_capsule_vs_structures(
            &structure_colliders,
            &mut pos.0,
            None,
            NPC_RADIUS,
            NPC_HEIGHT,
        );

        // Only snap UP if NPC fell below terrain.
        let ground_y = terrain.generator.get_height(pos.0.x, pos.0.z);
        let min_y = ground_y + shared::ground_clearance_center();
        if pos.0.y < min_y {
            pos.0.y = min_y;
        }
    }
}

/// Resolve vehicle collisions against static colliders (server-authoritative).
pub fn resolve_vehicle_static_collisions(
    derived: Option<Res<DerivedColliderLibrary>>,
    colliders: Res<StaticColliders>,
    structure_colliders: Res<StructureColliders>,
    mut vehicles: Query<&mut VehicleState>,
) {
    let Some(derived) = derived else { return };

    // Approximate bike footprint as a circle in XZ.
    let bike_radius =
        ((motorbike::SIZE.0 * 0.5).powi(2) + (motorbike::SIZE.2 * 0.5).powi(2)).sqrt();
    let bike_height = motorbike::SIZE.1;

    // Hover bikes skip small obstacles (rocks) - only collide with trees and large props
    // This makes the bike feel like it's actually hovering over terrain debris
    const MIN_HOVER_COLLISION_RADIUS: f32 = 1.2;

    for mut state in vehicles.iter_mut() {
        // Avoid double mutable borrow of `state` fields (Bevy `Mut<T>` borrow rules).
        let mut pos = state.position;
        let mut vel = state.velocity;

        // Resolve against props
        resolve_vehicle_vs_static(
            &derived,
            &colliders,
            &mut pos,
            Some(&mut vel),
            bike_radius,
            bike_height,
            MIN_HOVER_COLLISION_RADIUS,
        );

        // Resolve against structures
        resolve_capsule_vs_structures(
            &structure_colliders,
            &mut pos,
            Some(&mut vel),
            bike_radius,
            bike_height,
        );

        state.position = pos;
        state.velocity = vel;
    }
}

fn unload_chunk(colliders: &mut StaticColliders, chunk: ChunkCoord) {
    colliders.loaded_chunks.remove(&chunk);

    let Some(ids) = colliders.chunk_instances.remove(&chunk) else { return };
    for id in ids {
        if let Some(inst) = colliders.instances.remove(&id) {
            if let Some(cell_list) = colliders.cells.get_mut(&inst.cell) {
                cell_list.retain(|x| *x != id);
                if cell_list.is_empty() {
                    colliders.cells.remove(&inst.cell);
                }
            }
        }
    }
}

fn load_chunk(
    terrain: &WorldTerrain,
    library: &BakedColliderLibrary,
    colliders: &mut StaticColliders,
    chunk: ChunkCoord,
) {
    // Generate deterministic prop spawns for this chunk.
    let spawns = shared::generate_chunk_prop_spawns(&terrain.generator, chunk);

    let mut ids = Vec::new();
    for spawn in spawns {
        // Only index props that have baked colliders.
        if !library.by_kind.contains_key(&spawn.kind) {
            continue;
        }

        let id = colliders.next_id;
        colliders.next_id = colliders.next_id.wrapping_add(1);

        let cell = cell_key(spawn.position.x, spawn.position.z);
        let inst = StaticColliderInstance {
            kind: spawn.kind,
            position: spawn.position,
            rotation: spawn.rotation,
            scale: spawn.scale,
            cell,
        };

        colliders.instances.insert(id, inst);
        colliders.cells.entry(cell).or_default().push(id);
        ids.push(id);
    }

    colliders.loaded_chunks.insert(chunk);
    colliders.chunk_instances.insert(chunk, ids);
}

fn derive_collider(baked: &shared::BakedCollider) -> Option<DerivedCollider> {
    match baked {
        shared::BakedCollider::ConvexHull { points } => {
            if points.len() < 4 {
                return None;
            }

            let mut r2 = 0.0f32;
            let hull_vertices: Vec<Vec3> = points.iter()
                .map(|p| {
                    r2 = r2.max(p[0] * p[0] + p[1] * p[1] + p[2] * p[2]); // 3D bounding radius
                    Vec3::new(p[0], p[1], p[2])
                })
                .collect();

            // Triangulate the convex hull faces
            let hull_faces = triangulate_convex_hull(&hull_vertices);
            
            if hull_faces.is_empty() {
                warn!("Failed to triangulate convex hull with {} vertices", hull_vertices.len());
                return None;
            }

            Some(DerivedCollider {
                bounding_radius: r2.sqrt(),
                hull_faces,
            })
        }
    }
}

/// Triangulate a convex hull from its vertices.
/// Uses quickhull-style approach to find faces with outward normals.
fn triangulate_convex_hull(vertices: &[Vec3]) -> Vec<HullFace> {
    if vertices.len() < 4 {
        return vec![];
    }

    // Find centroid for determining outward normals
    let centroid = vertices.iter().fold(Vec3::ZERO, |a, &b| a + b) / vertices.len() as f32;
    
    // Use gift-wrapping/incremental approach to build faces
    // For a convex hull, we can use a simple approach: for each triple of non-collinear points,
    // check if it forms a valid face (all other points are on one side)
    
    let mut faces = Vec::new();
    let n = vertices.len();
    
    // Try all triangles and keep valid hull faces
    for i in 0..n {
        for j in (i + 1)..n {
            for k in (j + 1)..n {
                let v0 = vertices[i];
                let v1 = vertices[j];
                let v2 = vertices[k];
                
                // Compute face normal
                let e1 = v1 - v0;
                let e2 = v2 - v0;
                let mut normal = e1.cross(e2);
                let len = normal.length();
                if len < 1e-6 {
                    continue; // Degenerate triangle
                }
                normal /= len;
                
                // d value for plane equation
                let d = normal.dot(v0);
                
                // Check if all other vertices are on the same side (or on) this plane
                let mut all_behind = true;
                let mut any_in_front = false;
                
                for m in 0..n {
                    if m == i || m == j || m == k {
                        continue;
                    }
                    let dist = normal.dot(vertices[m]) - d;
                    if dist > 1e-4 {
                        any_in_front = true;
                        all_behind = false;
                        break;
                    }
                }
                
                if !all_behind || any_in_front {
                    // Try flipping normal
                    let flipped_normal = -normal;
                    let flipped_d = -d;
                    
                    let mut all_behind_flipped = true;
                    for m in 0..n {
                        if m == i || m == j || m == k {
                            continue;
                        }
                        let dist = flipped_normal.dot(vertices[m]) - flipped_d;
                        if dist > 1e-4 {
                            all_behind_flipped = false;
                            break;
                        }
                    }
                    
                    if all_behind_flipped {
                        // Use flipped version - also flip vertex winding
                        faces.push(HullFace {
                            vertices: [v0, v2, v1],
                            normal: flipped_normal,
                            d: flipped_d,
                        });
                    }
                } else {
                    // Ensure normal points outward (away from centroid)
                    let to_centroid = centroid - v0;
                    if normal.dot(to_centroid) > 0.0 {
                        // Normal points toward centroid, flip it
                        faces.push(HullFace {
                            vertices: [v0, v2, v1],
                            normal: -normal,
                            d: -d,
                        });
                    } else {
                        faces.push(HullFace {
                            vertices: [v0, v1, v2],
                            normal,
                            d,
                        });
                    }
                }
            }
        }
    }
    
    // Remove duplicate faces (same vertices in any order)
    faces.dedup_by(|a, b| {
        let mut a_verts: Vec<_> = a.vertices.iter().map(|v| 
            ((v.x * 1000.0) as i32, (v.y * 1000.0) as i32, (v.z * 1000.0) as i32)
        ).collect();
        let mut b_verts: Vec<_> = b.vertices.iter().map(|v| 
            ((v.x * 1000.0) as i32, (v.y * 1000.0) as i32, (v.z * 1000.0) as i32)
        ).collect();
        a_verts.sort();
        b_verts.sort();
        a_verts == b_verts
    });
    
    faces
}

/// Vehicle-specific collision that skips small obstacles (hover over rocks)
fn resolve_vehicle_vs_static(
    derived: &DerivedColliderLibrary,
    colliders: &StaticColliders,
    pos: &mut Vec3,
    mut velocity: Option<&mut Vec3>,
    radius: f32,
    height: f32,
    min_obstacle_radius: f32, // Skip obstacles smaller than this (hover over them)
) {
    let half_h = height * 0.5;
    
    for _ in 0..4 {
        let mut moved = false;
        let candidates = nearby_instance_ids(colliders, *pos, radius + 6.0);

        for id in candidates {
            let Some(inst) = colliders.instances.get(&id) else { continue };
            let Some(shape) = derived.by_kind.get(&inst.kind) else { continue };

            // Skip small obstacles - hover bikes glide over rocks
            let obstacle_size = shape.bounding_radius * inst.scale;
            if obstacle_size < min_obstacle_radius {
                continue;
            }

            // Broad-phase: bounding sphere rejection
            let bounding_r = obstacle_size.max(0.05);
            let to_prop = *pos - inst.position;
            let dist2 = to_prop.length_squared();
            let max_dist = radius + bounding_r + half_h;
            if dist2 >= max_dist * max_dist {
                continue;
            }

            // Narrow-phase: capsule vs 3D convex hull
            let sphere_positions = [
                *pos - Vec3::Y * (half_h - radius).max(0.0),
                *pos,
                *pos + Vec3::Y * (half_h - radius).max(0.0),
            ];
            
            let mut best_penetration = 0.0f32;
            let mut best_normal = Vec3::ZERO;
            
            for sphere_pos in sphere_positions {
                if let Some((pen, normal)) = sphere_vs_convex_hull_3d(
                    sphere_pos,
                    radius,
                    &shape.hull_faces,
                    inst.position,
                    inst.rotation,
                    inst.scale,
                ) {
                    if pen > best_penetration {
                        best_penetration = pen;
                        best_normal = normal;
                    }
                }
            }
            
            if best_penetration <= 0.0 {
                continue;
            }

            // Push out along normal
            let push = best_normal * best_penetration;
            pos.x += push.x;
            pos.y += push.y.max(0.0);
            pos.z += push.z;
            
            if let Some(v) = velocity.as_deref_mut() {
                let vn = v.dot(best_normal);
                if vn < 0.0 {
                    *v -= best_normal * vn;
                }
            }
            
            moved = true;
        }

        if !moved {
            break;
        }
    }
}

fn resolve_capsule_vs_static(
    derived: &DerivedColliderLibrary,
    colliders: &StaticColliders,
    pos: &mut Vec3,
    mut velocity: Option<&mut Vec3>,
    radius: f32,
    height: f32,
    _step_up_height: f32, // Reserved for future use
) {
            let half_h = height * 0.5;
    
    // Iterative push-out to handle multiple overlaps.
    for _ in 0..4 {
        let mut moved = false;
        let candidates = nearby_instance_ids(colliders, *pos, radius + 6.0);

        for id in candidates {
            let Some(inst) = colliders.instances.get(&id) else { continue };
            let Some(shape) = derived.by_kind.get(&inst.kind) else { continue };

            // Broad-phase: bounding sphere rejection
            let bounding_r = (shape.bounding_radius * inst.scale).max(0.05);
            let to_prop = *pos - inst.position;
            let dist2 = to_prop.length_squared();
            let max_dist = radius + bounding_r + half_h;
            if dist2 >= max_dist * max_dist {
                continue;
            }

            // Narrow-phase: capsule vs 3D convex hull
            // We approximate capsule as multiple spheres along its axis
            let sphere_positions = [
                *pos - Vec3::Y * (half_h - radius).max(0.0),  // Bottom sphere
                *pos,                                          // Middle sphere
                *pos + Vec3::Y * (half_h - radius).max(0.0),  // Top sphere
            ];
            
            let mut best_penetration = 0.0f32;
            let mut best_normal = Vec3::ZERO;
            
            for sphere_pos in sphere_positions {
                if let Some((pen, normal)) = sphere_vs_convex_hull_3d(
                    sphere_pos,
                    radius,
                    &shape.hull_faces,
                    inst.position,
                    inst.rotation,
                    inst.scale,
                ) {
                    if pen > best_penetration {
                        best_penetration = pen;
                        best_normal = normal;
                    }
                }
            }
            
            if best_penetration <= 0.0 {
                continue;
            }

            // Check if this is a surface we can step up onto
            // If the normal is mostly upward and we're hitting a low surface
            let is_walkable_slope = best_normal.y > 0.5; // ~60 degree slope or less
            
            if is_walkable_slope {
                // Push along the surface normal (allows climbing slopes)
                let push = best_normal * best_penetration;
                pos.x += push.x;
                pos.y += push.y;
                pos.z += push.z;

                if let Some(v) = velocity.as_deref_mut() {
                    // Project velocity onto the surface (slide along it)
                    let vn = v.dot(best_normal);
                    if vn < 0.0 {
                        *v -= best_normal * vn;
                }
                }
            } else {
                // Steep surface - push out along normal but preserve some vertical
                let push = best_normal * best_penetration;
                pos.x += push.x;
                pos.y += push.y.max(0.0); // Don't push down
                pos.z += push.z;

                if let Some(v) = velocity.as_deref_mut() {
                    let vn = v.dot(best_normal);
                    if vn < 0.0 {
                        *v -= best_normal * vn;
                    }
                    }
                }

                moved = true;
        }

        if !moved {
            break;
        }
    }
}

/// Test sphere vs scaled and rotated 3D convex hull. Returns (penetration_depth, push_normal) if colliding.
fn sphere_vs_convex_hull_3d(
    sphere_center: Vec3,
    radius: f32,
    faces: &[HullFace],
    hull_origin: Vec3,
    hull_rotation: Quat,
    scale: f32,
) -> Option<(f32, Vec3)> {
    if faces.is_empty() {
        return None;
    }

    // Transform sphere to hull local space:
    // 1. Translate to origin
    // 2. Rotate by inverse rotation
    // 3. Scale
    let inv_rotation = hull_rotation.inverse();
    let local_center = inv_rotation * (sphere_center - hull_origin) / scale;
    let local_radius = radius / scale;

    // Find the closest point on the hull to the sphere center
    let mut min_dist = f32::INFINITY;
    let mut closest_normal = Vec3::ZERO;
    let mut inside_all_faces = true;

    for face in faces {
        // Signed distance from sphere center to face plane
        let dist_to_plane = face.normal.dot(local_center) - face.d;
        
        if dist_to_plane > local_radius {
            // Sphere is completely outside this face - no collision with hull
            return None;
        }
        
        if dist_to_plane > 0.0 {
            inside_all_faces = false;
        }

        // Project sphere center onto face plane
        let projected = local_center - face.normal * dist_to_plane;
        
        // Check if projected point is inside the triangle
        let closest_on_face = closest_point_on_triangle(projected, &face.vertices);
        let to_sphere = local_center - closest_on_face;
        let dist = to_sphere.length();
        
        if dist < min_dist {
            min_dist = dist;
            if dist > 1e-6 {
                closest_normal = to_sphere / dist;
            } else {
                closest_normal = face.normal;
            }
        }
    }

    // Check for collision
    if inside_all_faces {
        // Sphere center is inside hull - find closest face to push out through
        let mut closest_face_dist = f32::INFINITY;
        for face in faces {
            let dist = (face.normal.dot(local_center) - face.d).abs();
            if dist < closest_face_dist {
                closest_face_dist = dist;
                closest_normal = face.normal;
            }
        }
        let penetration = (closest_face_dist + local_radius) * scale;
        // Transform normal back to world space
        let world_normal = hull_rotation * closest_normal;
        return Some((penetration, world_normal));
    }
    
    if min_dist < local_radius {
        let penetration = (local_radius - min_dist) * scale;
        // Transform normal back to world space
        let world_normal = hull_rotation * closest_normal;
        Some((penetration, world_normal))
    } else {
        None
    }
}

/// Find closest point on a triangle to a given point.
fn closest_point_on_triangle(p: Vec3, tri: &[Vec3; 3]) -> Vec3 {
    let a = tri[0];
    let b = tri[1];
    let c = tri[2];
    
    // Check if P in vertex region outside A
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    
    // Check if P in vertex region outside B
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    
    // Check if P in edge region of AB
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return a + ab * v;
    }
    
    // Check if P in vertex region outside C
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    
    // Check if P in edge region of AC
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return a + ac * w;
    }
    
    // Check if P in edge region of BC
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return b + (c - b) * w;
    }
    
    // P inside face region
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    a + ab * v + ac * w
}

fn nearby_instance_ids(colliders: &StaticColliders, pos: Vec3, radius: f32) -> Vec<u32> {
    let (cx, cz) = cell_key(pos.x, pos.z);
    let cells = (radius / COLLIDER_CELL_SIZE).ceil() as i32 + 1;
    let mut out = Vec::new();

    for dx in -cells..=cells {
        for dz in -cells..=cells {
            if let Some(list) = colliders.cells.get(&(cx + dx, cz + dz)) {
                out.extend(list.iter().copied());
            }
        }
    }

    out
}

fn nearby_structure_ids(colliders: &StructureColliders, pos: Vec3, radius: f32) -> Vec<u32> {
    let (cx, cz) = cell_key(pos.x, pos.z);
    let cells = (radius / COLLIDER_CELL_SIZE).ceil() as i32 + 1;
    let mut out = Vec::new();

    for dx in -cells..=cells {
        for dz in -cells..=cells {
            if let Some(list) = colliders.cells.get(&(cx + dx, cz + dz)) {
                out.extend(list.iter().copied());
            }
        }
    }

    out
}

/// Resolve capsule vs structure colliders
fn resolve_capsule_vs_structures(
    colliders: &StructureColliders,
    pos: &mut Vec3,
    mut velocity: Option<&mut Vec3>,
    radius: f32,
    height: f32,
) {
    let half_h = height * 0.5;

    for _ in 0..4 {
        let mut moved = false;
        let candidates = nearby_structure_ids(colliders, *pos, radius + 15.0);

        for id in candidates {
            let Some(inst) = colliders.instances.get(&id) else { continue };
            let collider = inst.kind.collider();

            // Broad-phase: bounding sphere rejection
            let bounding_r = get_structure_bounding_radius(&collider, inst.scale);
            let to_structure = *pos - inst.position;
            let dist2 = to_structure.length_squared();
            let max_dist = radius + bounding_r + half_h;
            if dist2 >= max_dist * max_dist {
                continue;
            }

            // Narrow-phase: capsule vs structure shape
            // Approximate capsule as multiple spheres
            let sphere_positions = [
                *pos - Vec3::Y * (half_h - radius).max(0.0),  // Bottom
                *pos,                                          // Middle
                *pos + Vec3::Y * (half_h - radius).max(0.0),  // Top
            ];

            let mut best_penetration = 0.0f32;
            let mut best_normal = Vec3::ZERO;

            for sphere_pos in sphere_positions {
                if let Some((pen, normal)) = sphere_vs_structure(
                    sphere_pos,
                    radius,
                    &collider,
                    inst.position,
                    inst.rotation,
                    inst.scale,
                ) {
                    if pen > best_penetration {
                        best_penetration = pen;
                        best_normal = normal;
                    }
                }
            }

            if best_penetration <= 0.0 {
                continue;
            }

            // Push out
            let push = best_normal * best_penetration;
            pos.x += push.x;
            pos.y += push.y.max(0.0);
            pos.z += push.z;

            if let Some(v) = velocity.as_deref_mut() {
                let vn = v.dot(best_normal);
                if vn < 0.0 {
                    *v -= best_normal * vn;
                }
            }

            moved = true;
        }

        if !moved {
            break;
        }
    }
}

fn get_structure_bounding_radius(collider: &StructureCollider, scale: f32) -> f32 {
    match collider {
        StructureCollider::Dome { radius, height } => {
            ((radius * radius) + (height * height)).sqrt() * scale
        }
        StructureCollider::Cylinder { radius, height } => {
            ((radius * radius) + (height * 0.5 * height * 0.5)).sqrt() * scale
        }
        StructureCollider::Box { half_extents } => {
            half_extents.length() * scale
        }
        StructureCollider::Arch { width, height, depth, .. } => {
            Vec3::new(width * 0.5, *height, depth * 0.5).length() * scale
        }
    }
}

/// Test sphere against a structure collider
fn sphere_vs_structure(
    sphere_center: Vec3,
    sphere_radius: f32,
    collider: &StructureCollider,
    structure_pos: Vec3,
    structure_rot: Quat,
    scale: f32,
) -> Option<(f32, Vec3)> {
    // Transform sphere to structure local space
    let inv_rot = structure_rot.inverse();
    let local_center = inv_rot * (sphere_center - structure_pos) / scale;
    let local_radius = sphere_radius / scale;

    let result = match collider {
        StructureCollider::Dome { radius, height } => {
            sphere_vs_dome(local_center, local_radius, *radius, *height)
        }
        StructureCollider::Cylinder { radius, height } => {
            sphere_vs_cylinder(local_center, local_radius, *radius, *height)
        }
        StructureCollider::Box { half_extents } => {
            sphere_vs_box(local_center, local_radius, *half_extents)
        }
        StructureCollider::Arch { width, height, depth, thickness } => {
            sphere_vs_arch(local_center, local_radius, *width, *height, *depth, *thickness)
        }
    };

    result.map(|(pen, normal)| {
        // Transform penetration and normal back to world space
        (pen * scale, structure_rot * normal)
    })
}

/// Sphere vs hemisphere dome (sitting on ground)
/// Matches the mesh generation which has a flattened top: y = height * phi.cos() * (1.0 - phi.cos() * 0.3)
fn sphere_vs_dome(
    center: Vec3,
    radius: f32,
    dome_radius: f32,
    dome_height: f32,
) -> Option<(f32, Vec3)> {
    // Check if sphere is below dome or too far above
    if center.y < -radius {
        return None;
    }
    
    // The visual dome has a flattened top, so effective height is about 70% at the peak
    // Match the mesh formula: y = height * phi.cos() * (1.0 - phi.cos() * 0.3)
    // At the very top (phi=0), this gives height * 0.7
    let effective_height = dome_height * 0.7;
    
    // Quick bounds check
    let xz_dist = (center.x * center.x + center.z * center.z).sqrt();
    if xz_dist > dome_radius + radius && center.y > effective_height + radius {
        return None;
    }
    
    // For collision, treat as a scaled hemisphere with the effective height
    let height_ratio = effective_height / dome_radius;
    
    // Scaled coordinates for spherical check  
    let scaled_center = Vec3::new(center.x, center.y / height_ratio.max(0.01), center.z);
    let dist_to_origin = scaled_center.length();
    
    // Check if inside the dome volume
    if dist_to_origin < dome_radius + radius {
        if dist_to_origin < 0.001 {
            // At center - push straight up
            return Some((effective_height + radius - center.y, Vec3::Y));
        }
        
        // Normal direction in scaled space
        let scaled_normal = scaled_center / dist_to_origin;
        // Transform normal back to account for height scaling
        let normal = Vec3::new(scaled_normal.x, scaled_normal.y / height_ratio.max(0.01), scaled_normal.z).normalize();
        
        // Calculate penetration
        let penetration = (dome_radius - dist_to_origin + radius).max(0.0);
        
        if penetration > 0.0 && center.y >= 0.0 {
            return Some((penetration, normal));
        }
    }

    None
}

/// Sphere vs cylinder (standing upright)
fn sphere_vs_cylinder(
    center: Vec3,
    radius: f32,
    cyl_radius: f32,
    cyl_height: f32,
) -> Option<(f32, Vec3)> {
    // Cylinder from y=0 to y=height
    let cyl_half_h = cyl_height * 0.5;
    let cyl_center_y = cyl_half_h;

    // Check if in vertical range
    let dy = center.y - cyl_center_y;
    if dy.abs() > cyl_half_h + radius {
        return None;
    }

    // XZ distance
    let xz_dist = (center.x * center.x + center.z * center.z).sqrt();

    // Check against infinite cylinder
    if xz_dist < cyl_radius + radius {
        // Check if hitting the side
        if center.y > 0.0 && center.y < cyl_height {
            let penetration = cyl_radius + radius - xz_dist;
            if penetration > 0.0 {
                let normal = if xz_dist > 0.001 {
                    Vec3::new(center.x / xz_dist, 0.0, center.z / xz_dist)
                } else {
                    Vec3::X
                };
                return Some((penetration, normal));
            }
        }

        // Check top cap
        if center.y > cyl_height - radius && xz_dist < cyl_radius {
            let pen_top = center.y + radius - cyl_height;
            if pen_top > 0.0 {
                return Some((pen_top, Vec3::Y));
            }
        }
    }

    None
}

/// Sphere vs axis-aligned box
fn sphere_vs_box(
    center: Vec3,
    radius: f32,
    half_extents: Vec3,
) -> Option<(f32, Vec3)> {
    // Find closest point on box to sphere center
    let closest = Vec3::new(
        center.x.clamp(-half_extents.x, half_extents.x),
        center.y.clamp(0.0, half_extents.y * 2.0), // Box sits on ground (y=0 to y=2*half_y)
        center.z.clamp(-half_extents.z, half_extents.z),
    );

    let to_sphere = center - closest;
    let dist = to_sphere.length();

    if dist < radius && dist > 0.001 {
        let normal = to_sphere / dist;
        let penetration = radius - dist;
        return Some((penetration, normal));
    } else if dist <= 0.001 {
        // Sphere center inside box - push out through nearest face
        let dx = half_extents.x - center.x.abs();
        let dy_bottom = center.y;
        let dy_top = half_extents.y * 2.0 - center.y;
        let dz = half_extents.z - center.z.abs();

        let min_dist = dx.min(dy_bottom).min(dy_top).min(dz);
        
        let normal = if min_dist == dx {
            Vec3::new(center.x.signum(), 0.0, 0.0)
        } else if min_dist == dy_bottom {
            -Vec3::Y
        } else if min_dist == dy_top {
            Vec3::Y
        } else {
            Vec3::new(0.0, 0.0, center.z.signum())
        };

        return Some((min_dist + radius, normal));
    }

    None
}

/// Sphere vs arch (simplified as two pillars + top bar)
fn sphere_vs_arch(
    center: Vec3,
    radius: f32,
    width: f32,
    height: f32,
    depth: f32,
    thickness: f32,
) -> Option<(f32, Vec3)> {
    let half_width = width * 0.5;
    let half_depth = depth * 0.5;
    let pillar_height = height * 0.6;

    // Check left pillar
    let left_center = Vec3::new(-half_width + thickness * 0.5, pillar_height * 0.5, 0.0);
    let left_half = Vec3::new(thickness * 0.5, pillar_height * 0.5, half_depth);
    if let Some(result) = sphere_vs_box(center - left_center + Vec3::new(0.0, left_half.y, 0.0), radius, left_half) {
        return Some(result);
    }

    // Check right pillar
    let right_center = Vec3::new(half_width - thickness * 0.5, pillar_height * 0.5, 0.0);
    let right_half = Vec3::new(thickness * 0.5, pillar_height * 0.5, half_depth);
    if let Some(result) = sphere_vs_box(center - right_center + Vec3::new(0.0, right_half.y, 0.0), radius, right_half) {
        return Some(result);
    }

    // Check top bar (simplified)
    if center.y > pillar_height - radius && center.y < height + radius {
        let xz_dist = (center.x * center.x + center.z * center.z).sqrt();
        if xz_dist < half_width && center.z.abs() < half_depth {
            let pen = center.y + radius - height;
            if pen > 0.0 && pen < radius * 2.0 {
                return Some((pen, Vec3::Y));
            }
        }
    }

    None
}