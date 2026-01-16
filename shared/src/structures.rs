//! Desert settlement structures - Dune-inspired buildings
//!
//! Procedurally generated structures that spawn in desert settlement zones.
//! These are spawned deterministically based on world seed.

use bevy::prelude::*;
use noise::{NoiseFn, Perlin};
use serde::{Deserialize, Serialize};

use crate::building::BuildingType;
use crate::terrain::{ChunkCoord, SettlementInfo, TerrainGenerator, CHUNK_SIZE, WORLD_SEED};

/// Types of desert structures (Dune-inspired)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DesertStructureKind {
    /// Small dome dwelling (3-4m radius)
    SmallDome,
    /// Large central dome (6-8m radius)
    LargeDome,
    /// Curved defensive/boundary wall segment
    CurvedWall,
    /// Tall cylindrical watchtower
    WatchTower,
    /// Entrance archway
    Archway,
    /// Cylindrical storage silo
    StorageSilo,
    /// Small decorative lamp post
    DesertLamp,
}

impl DesertStructureKind {
    /// Get the base radius of this structure type (used for placement)
    pub fn base_radius(&self) -> f32 {
        match self {
            DesertStructureKind::SmallDome => 3.5,
            DesertStructureKind::LargeDome => 7.0,
            DesertStructureKind::CurvedWall => 2.0,
            DesertStructureKind::WatchTower => 2.5,
            DesertStructureKind::Archway => 3.0,
            DesertStructureKind::StorageSilo => 2.0,
            DesertStructureKind::DesertLamp => 0.3,
        }
    }

    /// Get the height of this structure type
    pub fn height(&self) -> f32 {
        match self {
            DesertStructureKind::SmallDome => 4.0,
            DesertStructureKind::LargeDome => 8.0,
            DesertStructureKind::CurvedWall => 3.5,
            DesertStructureKind::WatchTower => 12.0,
            DesertStructureKind::Archway => 5.0,
            DesertStructureKind::StorageSilo => 5.0,
            DesertStructureKind::DesertLamp => 2.5,
        }
    }

    /// Stable string ID for serialization
    pub fn id(&self) -> &'static str {
        match self {
            DesertStructureKind::SmallDome => "small_dome",
            DesertStructureKind::LargeDome => "large_dome",
            DesertStructureKind::CurvedWall => "curved_wall",
            DesertStructureKind::WatchTower => "watch_tower",
            DesertStructureKind::Archway => "archway",
            DesertStructureKind::StorageSilo => "storage_silo",
            DesertStructureKind::DesertLamp => "desert_lamp",
        }
    }
}

/// A structure spawn instance
#[derive(Debug, Clone)]
pub struct StructureSpawn {
    pub kind: DesertStructureKind,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: f32,
    /// Which settlement this structure belongs to
    pub settlement_center: Vec2,
}

/// Collision shape for a structure
#[derive(Debug, Clone)]
pub enum StructureCollider {
    /// Hemisphere dome (radius, height)
    Dome { radius: f32, height: f32 },
    /// Cylinder (radius, height)
    Cylinder { radius: f32, height: f32 },
    /// Box (half extents)
    Box { half_extents: Vec3 },
    /// Arch (width, height, depth, thickness)
    Arch { width: f32, height: f32, depth: f32, thickness: f32 },
}

impl DesertStructureKind {
    /// Get the collision shape for this structure type at scale 1.0
    pub fn collider(&self) -> StructureCollider {
        match self {
            DesertStructureKind::SmallDome => StructureCollider::Dome {
                radius: 3.5,
                height: 4.0,
            },
            DesertStructureKind::LargeDome => StructureCollider::Dome {
                radius: 7.0,
                height: 8.0,
            },
            DesertStructureKind::CurvedWall => StructureCollider::Box {
                half_extents: Vec3::new(4.0, 1.75, 0.5),
            },
            DesertStructureKind::WatchTower => StructureCollider::Cylinder {
                radius: 2.5,
                height: 12.0,
            },
            DesertStructureKind::Archway => StructureCollider::Arch {
                width: 6.0,
                height: 5.0,
                depth: 2.0,
                thickness: 1.0,
            },
            DesertStructureKind::StorageSilo => StructureCollider::Cylinder {
                radius: 2.0,
                height: 5.0,
            },
            DesertStructureKind::DesertLamp => StructureCollider::Cylinder {
                radius: 0.3,
                height: 2.5,
            },
        }
    }
}

/// Generate all structures for a settlement
fn generate_settlement_structures(
    settlement: &SettlementInfo,
    structure_noise: &Perlin,
    placement_noise: &Perlin,
) -> Vec<StructureSpawn> {
    let mut structures = Vec::new();
    let center = settlement.center;
    let base_y = settlement.base_height;

    // Noise values for variety
    let variety = structure_noise.get([center.x as f64 * 0.05, center.y as f64 * 0.05]) as f32;
    let variety2 = structure_noise.get([center.y as f64 * 0.05, center.x as f64 * 0.05]) as f32;

    // 1. Place the main large dome at center
    structures.push(StructureSpawn {
        kind: DesertStructureKind::LargeDome,
        position: Vec3::new(center.x, base_y, center.y),
        rotation: Quat::from_rotation_y(variety * std::f32::consts::TAU),
        scale: 1.0 + variety.abs() * 0.1,
        settlement_center: center,
    });

    // 2. Place 4-7 small domes in a ring around center
    let num_small_domes = 4 + ((variety.abs() * 4.0) as usize).min(3);
    let dome_ring_radius = 18.0 + variety2.abs() * 5.0;

    for i in 0..num_small_domes {
        let angle = (i as f32 / num_small_domes as f32) * std::f32::consts::TAU
            + variety * 0.5; // Offset for variety

        // Add some jitter to position
        let jitter = placement_noise.get([
            (center.x + angle * 10.0) as f64 * 0.1,
            (center.y + angle * 10.0) as f64 * 0.1,
        ]) as f32;
        let radius = dome_ring_radius + jitter * 4.0;

        let pos_x = center.x + angle.cos() * radius;
        let pos_z = center.y + angle.sin() * radius;

        structures.push(StructureSpawn {
            kind: DesertStructureKind::SmallDome,
            position: Vec3::new(pos_x, base_y, pos_z),
            rotation: Quat::from_rotation_y(angle + std::f32::consts::PI), // Face center
            scale: 0.8 + jitter.abs() * 0.4,
            settlement_center: center,
        });
    }

    // 3. Place curved walls forming partial enclosure
    let num_walls = 5 + ((variety2.abs() * 4.0) as usize).min(3);
    let wall_ring_radius = 35.0 + variety.abs() * 8.0;
    let wall_arc_start = variety * std::f32::consts::PI; // Random starting point
    let wall_arc_span = std::f32::consts::PI * 1.2; // ~216 degrees of coverage

    for i in 0..num_walls {
        let t = i as f32 / num_walls as f32;
        let angle = wall_arc_start + t * wall_arc_span;

        let pos_x = center.x + angle.cos() * wall_ring_radius;
        let pos_z = center.y + angle.sin() * wall_ring_radius;

        structures.push(StructureSpawn {
            kind: DesertStructureKind::CurvedWall,
            position: Vec3::new(pos_x, base_y, pos_z),
            rotation: Quat::from_rotation_y(angle + std::f32::consts::FRAC_PI_2), // Perpendicular to radius
            scale: 1.0,
            settlement_center: center,
        });
    }

    // 4. Place archway at the gap in the walls (entrance)
    let archway_angle = wall_arc_start + wall_arc_span + std::f32::consts::FRAC_PI_4;
    let archway_radius = wall_ring_radius - 2.0;
    structures.push(StructureSpawn {
        kind: DesertStructureKind::Archway,
        position: Vec3::new(
            center.x + archway_angle.cos() * archway_radius,
            base_y,
            center.y + archway_angle.sin() * archway_radius,
        ),
        rotation: Quat::from_rotation_y(archway_angle + std::f32::consts::FRAC_PI_2),
        scale: 1.0,
        settlement_center: center,
    });

    // 5. Watchtower (~70% chance)
    if variety > -0.3 {
        let tower_angle = wall_arc_start + wall_arc_span * 0.5;
        let tower_radius = wall_ring_radius + 3.0;
        structures.push(StructureSpawn {
            kind: DesertStructureKind::WatchTower,
            position: Vec3::new(
                center.x + tower_angle.cos() * tower_radius,
                base_y,
                center.y + tower_angle.sin() * tower_radius,
            ),
            rotation: Quat::IDENTITY,
            scale: 1.0,
            settlement_center: center,
        });
    }

    // 6. Storage silos (2-4)
    let num_silos = 2 + ((variety2.abs() * 3.0) as usize).min(2);
    for i in 0..num_silos {
        let silo_angle = variety2 * std::f32::consts::TAU + (i as f32 * 1.5);
        let silo_radius = 10.0 + (i as f32 * 3.0);

        let jitter = placement_noise.get([
            (center.x + silo_angle * 20.0) as f64 * 0.1,
            (center.y + silo_angle * 20.0) as f64 * 0.1,
        ]) as f32;

        structures.push(StructureSpawn {
            kind: DesertStructureKind::StorageSilo,
            position: Vec3::new(
                center.x + silo_angle.cos() * silo_radius + jitter * 2.0,
                base_y,
                center.y + silo_angle.sin() * silo_radius + jitter * 2.0,
            ),
            rotation: Quat::IDENTITY,
            scale: 0.9 + jitter.abs() * 0.2,
            settlement_center: center,
        });
    }

    // 7. Desert lamps scattered throughout the settlement (6-10)
    let num_lamps = 6 + ((variety.abs() * 5.0) as usize).min(4);
    for i in 0..num_lamps {
        let lamp_noise = placement_noise.get([
            (center.x + i as f32 * 7.3) as f64 * 0.15,
            (center.y + i as f32 * 11.7) as f64 * 0.15,
        ]) as f32;
        let lamp_noise2 = placement_noise.get([
            (center.y + i as f32 * 13.1) as f64 * 0.15,
            (center.x + i as f32 * 5.9) as f64 * 0.15,
        ]) as f32;

        // Place lamps at various radii throughout the settlement
        let lamp_radius = 8.0 + (i as f32 * 4.0) + lamp_noise * 5.0;
        let lamp_angle = (i as f32 / num_lamps as f32) * std::f32::consts::TAU + lamp_noise2 * 0.8;

        structures.push(StructureSpawn {
            kind: DesertStructureKind::DesertLamp,
            position: Vec3::new(
                center.x + lamp_angle.cos() * lamp_radius,
                base_y,
                center.y + lamp_angle.sin() * lamp_radius,
            ),
            rotation: Quat::IDENTITY,
            scale: 0.9 + lamp_noise.abs() * 0.2,
            settlement_center: center,
        });
    }

    structures
}

/// Generate all structure spawns for a chunk
/// Returns structures that intersect with this chunk
pub fn generate_chunk_structures(terrain: &TerrainGenerator, chunk: ChunkCoord) -> Vec<StructureSpawn> {
    let settlements = terrain.get_settlements_near_chunk(chunk);

    if settlements.is_empty() {
        return Vec::new();
    }

    // Deterministic noise for structure placement
    let structure_noise = Perlin::new(WORLD_SEED.wrapping_add(9000));
    let placement_noise = Perlin::new(WORLD_SEED.wrapping_add(9500));

    let chunk_origin = chunk.world_pos();
    let chunk_min = Vec2::new(chunk_origin.x, chunk_origin.z);
    let chunk_max = Vec2::new(chunk_origin.x + CHUNK_SIZE, chunk_origin.z + CHUNK_SIZE);

    let mut out = Vec::new();

    for settlement in &settlements {
        let structures = generate_settlement_structures(settlement, &structure_noise, &placement_noise);

        // Filter to structures that intersect this chunk
        for structure in structures {
            let pos_2d = Vec2::new(structure.position.x, structure.position.z);
            let radius = structure.kind.base_radius() * structure.scale;

            // Simple AABB check for structure circle vs chunk rect
            let closest_x = pos_2d.x.clamp(chunk_min.x, chunk_max.x);
            let closest_z = pos_2d.y.clamp(chunk_min.y, chunk_max.y);
            let dist_sq = (pos_2d.x - closest_x).powi(2) + (pos_2d.y - closest_z).powi(2);

            if dist_sq <= radius * radius {
                out.push(structure);
            }
        }
    }

    out
}

/// Check if a position is inside any structure's footprint
/// Used to prevent prop spawning inside buildings
pub fn is_inside_structure(x: f32, z: f32, structures: &[StructureSpawn]) -> bool {
    for structure in structures {
        let dx = x - structure.position.x;
        let dz = z - structure.position.z;
        let dist_sq = dx * dx + dz * dz;
        let radius = structure.kind.base_radius() * structure.scale * 1.5; // Add padding

        if dist_sq < radius * radius {
            return true;
        }
    }
    false
}

/// Get all structures that could affect a given world position
pub fn get_structures_at(terrain: &TerrainGenerator, x: f32, z: f32) -> Vec<StructureSpawn> {
    let chunk = ChunkCoord::from_world_pos(Vec3::new(x, 0.0, z));

    // Check this chunk and neighbors (structures can span chunks)
    let mut all_structures = Vec::new();
    for dx in -1..=1 {
        for dz in -1..=1 {
            let neighbor = ChunkCoord::new(chunk.x + dx, chunk.z + dz);
            all_structures.extend(generate_chunk_structures(terrain, neighbor));
        }
    }

    all_structures
}

// =============================================================================
// MEDIEVAL TOWN GENERATION
// =============================================================================

/// A building spawn for the medieval town
#[derive(Debug, Clone)]
pub struct MedievalTownBuilding {
    pub building_type: BuildingType,
    pub position: Vec3,
    pub rotation: f32, // Y-axis rotation in radians
}

/// Town layout constants - exported for use by NPC spawning
pub const MEDIEVAL_BLOCK_SIZE: f32 = 45.0;
pub const MEDIEVAL_STREET_WIDTH: f32 = 8.0;
pub const MEDIEVAL_SPACING: f32 = MEDIEVAL_BLOCK_SIZE + MEDIEVAL_STREET_WIDTH; // 53m between block centers

/// Generate medieval town buildings in a grid pattern around a center point.
///
/// Layout: 3x3 grid of blocks with center block as town square (empty).
/// Each block contains 4-5 houses with proper spacing.
///
/// Returns ~35 buildings total.
pub fn generate_medieval_town(center: Vec3, seed: u32) -> Vec<MedievalTownBuilding> {
    let mut buildings = Vec::new();

    // Use deterministic noise for variety
    let noise = Perlin::new(seed.wrapping_add(12000));

    // Block offsets (3x3 grid, skip center for town square)
    let block_offsets: [(i32, i32); 8] = [
        (-1, -1), (0, -1), (1, -1),
        (-1,  0),          (1,  0),
        (-1,  1), (0,  1), (1,  1),
    ];

    // Blocks adjacent to center get larger houses (inner ring)
    let inner_blocks: [(i32, i32); 4] = [
        (0, -1), (-1, 0), (1, 0), (0, 1),
    ];

    for (bx, bz) in block_offsets.iter() {
        let block_center = Vec3::new(
            center.x + *bx as f32 * MEDIEVAL_SPACING,
            center.y,
            center.z + *bz as f32 * MEDIEVAL_SPACING,
        );

        let is_inner = inner_blocks.contains(&(*bx, *bz));

        // Generate houses within this block
        let block_buildings = generate_block_buildings(
            block_center,
            is_inner,
            &noise,
            *bx,
            *bz,
        );
        buildings.extend(block_buildings);
    }

    // Add 2 manors near the town square (special placement)
    // Manor 1: East side of square
    buildings.push(MedievalTownBuilding {
        building_type: BuildingType::House10,
        position: Vec3::new(center.x + 20.0, center.y, center.z),
        rotation: std::f32::consts::PI, // Face the square (west)
    });

    // Manor 2: West side of square
    buildings.push(MedievalTownBuilding {
        building_type: BuildingType::House10,
        position: Vec3::new(center.x - 20.0, center.y, center.z),
        rotation: 0.0, // Face the square (east)
    });

    buildings
}

/// Generate buildings within a single block
fn generate_block_buildings(
    block_center: Vec3,
    is_inner: bool,
    noise: &Perlin,
    block_x: i32,
    block_z: i32,
) -> Vec<MedievalTownBuilding> {
    let mut buildings = Vec::new();

    // Tighter house positions within block (relative to block center)
    // Houses placed closer together for more compact feel
    let house_positions: [(f32, f32, f32); 4] = [
        (-12.0, -12.0, std::f32::consts::FRAC_PI_4 * 5.0),        // SW corner, face SW
        ( 12.0, -12.0, -std::f32::consts::FRAC_PI_4),              // SE corner, face SE
        (-12.0,  12.0, std::f32::consts::FRAC_PI_4 * 3.0),        // NW corner, face NW
        ( 12.0,  12.0, std::f32::consts::FRAC_PI_4),               // NE corner, face NE
    ];

    for (i, (dx, dz, base_rot)) in house_positions.iter().enumerate() {
        // Smaller jitter for tighter layout
        let jitter_x = noise.get([
            (block_center.x + dx + i as f32 * 17.3) as f64 * 0.1,
            (block_center.z + dz) as f64 * 0.1,
        ]) as f32 * 1.5;
        let jitter_z = noise.get([
            (block_center.x + dx) as f64 * 0.1,
            (block_center.z + dz + i as f32 * 13.7) as f64 * 0.1,
        ]) as f32 * 1.5;

        // Noise for rotation variation
        let rot_jitter = noise.get([
            (block_x as f64 + i as f64) * 0.5,
            (block_z as f64 + i as f64) * 0.5,
        ]) as f32 * 0.2;

        // Select house type based on position and noise
        let type_noise = noise.get([
            (block_center.x + dx * 2.0) as f64 * 0.05,
            (block_center.z + dz * 2.0) as f64 * 0.05,
        ]) as f32;

        let building_type = if is_inner && type_noise > 0.3 {
            // Inner blocks have chance for larger houses
            BuildingType::House09
        } else {
            // Select from small house types based on noise
            match ((type_noise * 4.0).abs() as i32) % 4 {
                0 => BuildingType::House01,
                1 => BuildingType::House02,
                2 => BuildingType::House03,
                _ => BuildingType::House04,
            }
        };

        buildings.push(MedievalTownBuilding {
            building_type,
            position: Vec3::new(
                block_center.x + dx + jitter_x,
                block_center.y,
                block_center.z + dz + jitter_z,
            ),
            rotation: base_rot + rot_jitter,
        });
    }

    // Add a 5th house in some blocks (center-ish position)
    let extra_house_noise = noise.get([
        block_x as f64 * 0.7,
        block_z as f64 * 0.7,
    ]) as f32;

    if extra_house_noise > -0.2 {
        let extra_rot = extra_house_noise * std::f32::consts::TAU;
        let extra_type = if is_inner && extra_house_noise > 0.5 {
            BuildingType::House09
        } else {
            match ((extra_house_noise * 4.0).abs() as i32) % 4 {
                0 => BuildingType::House01,
                1 => BuildingType::House02,
                2 => BuildingType::House03,
                _ => BuildingType::House04,
            }
        };

        buildings.push(MedievalTownBuilding {
            building_type: extra_type,
            position: Vec3::new(
                block_center.x + extra_house_noise * 5.0,
                block_center.y,
                block_center.z + (1.0 - extra_house_noise.abs()) * 5.0 * extra_house_noise.signum(),
            ),
            rotation: extra_rot,
        });
    }

    buildings
}
