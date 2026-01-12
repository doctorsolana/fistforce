//! Procedural terrain generation with biomes
//! Uses deterministic noise so server and client generate identical terrain from seed
//!
//! Scale: 1 unit = 1 meter
//! - Player is 1.8m tall (human scale)
//! - Chunks are 64m x 64m
//! - Desert biome spans ~500-1000m before transitioning

use bevy::prelude::*;
use noise::{NoiseFn, Perlin};
use serde::{Deserialize, Serialize};

/// World generation seed - same seed = same world
pub const WORLD_SEED: u32 = 42;

/// Chunk size in world units (meters)
pub const CHUNK_SIZE: f32 = 64.0;
/// Number of vertices per chunk side (resolution)
pub const CHUNK_RESOLUTION: usize = 33;
/// Spacing between vertices
pub const VERTEX_SPACING: f32 = CHUNK_SIZE / (CHUNK_RESOLUTION - 1) as f32;

/// Maximum terrain height variation (meters)
pub const MAX_HEIGHT: f32 = 25.0;
/// Base height offset
pub const BASE_HEIGHT: f32 = 0.0;

/// Biome types available in the world
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Biome {
    Desert,
    Grasslands,
    Natureland,
}

impl Biome {
    /// Get the color for this biome
    pub fn color(&self) -> Color {
        match self {
            // Warm golden sand
            Biome::Desert => Color::srgb(0.85, 0.75, 0.55),
            // Lush green grass
            Biome::Grasslands => Color::srgb(0.35, 0.55, 0.25),
            // Stylized vibrant forest floor - rich earthy brown with moss tint
            Biome::Natureland => Color::srgb(0.28, 0.42, 0.22),
        }
    }

    /// Get a secondary accent color for variation
    pub fn accent_color(&self) -> Color {
        match self {
            Biome::Desert => Color::srgb(0.90, 0.80, 0.60),
            Biome::Grasslands => Color::srgb(0.40, 0.60, 0.30),
            Biome::Natureland => Color::srgb(0.32, 0.48, 0.26),
        }
    }
}

/// Chunk coordinate (integer grid position)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component, Serialize, Deserialize)]
pub struct ChunkCoord {
    pub x: i32,
    pub z: i32,
}

impl ChunkCoord {
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// Convert world position to chunk coordinate
    pub fn from_world_pos(pos: Vec3) -> Self {
        Self {
            x: (pos.x / CHUNK_SIZE).floor() as i32,
            z: (pos.z / CHUNK_SIZE).floor() as i32,
        }
    }

    /// Get the world position of the chunk's corner (min x, min z)
    pub fn world_pos(&self) -> Vec3 {
        Vec3::new(
            self.x as f32 * CHUNK_SIZE,
            0.0,
            self.z as f32 * CHUNK_SIZE,
        )
    }

    /// Get chunks in a radius around this chunk
    pub fn chunks_in_radius(&self, radius: i32) -> Vec<ChunkCoord> {
        let mut chunks = Vec::new();
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                chunks.push(ChunkCoord::new(self.x + dx, self.z + dz));
            }
        }
        chunks
    }
}

/// Settlement zone radius in meters
pub const SETTLEMENT_RADIUS: f32 = 50.0;
/// Settlement spawn spacing (grid cells)
pub const SETTLEMENT_GRID_SIZE: f32 = 400.0;
/// Minimum distance from world spawn to place settlements
pub const SETTLEMENT_MIN_SPAWN_DIST: f32 = 200.0;

/// Information about a desert settlement zone
#[derive(Debug, Clone, Copy)]
pub struct SettlementInfo {
    /// Center position of the settlement in world coordinates
    pub center: Vec2,
    /// Radius of the flattened zone
    pub radius: f32,
    /// Base height of the flattened terrain
    pub base_height: f32,
}

/// Terrain generator using Perlin noise
pub struct TerrainGenerator {
    height_noise: Perlin,
    biome_noise: Perlin,
    biome_noise_2: Perlin, // Secondary noise for natureland placement
    dune_noise: Perlin,
    detail_noise: Perlin,
    settlement_noise: Perlin, // Noise for determining settlement locations
    #[allow(dead_code)]
    seed: u32,
}

impl TerrainGenerator {
    pub fn new(seed: u32) -> Self {
        Self {
            height_noise: Perlin::new(seed),
            biome_noise: Perlin::new(seed.wrapping_add(1000)),
            biome_noise_2: Perlin::new(seed.wrapping_add(1500)),
            dune_noise: Perlin::new(seed.wrapping_add(2000)),
            detail_noise: Perlin::new(seed.wrapping_add(3000)),
            settlement_noise: Perlin::new(seed.wrapping_add(8000)),
            seed,
        }
    }

    /// Check if a grid cell should have a settlement
    /// Uses deterministic noise to decide (~15% of valid desert cells)
    fn cell_has_settlement(&self, cell_x: i32, cell_z: i32) -> bool {
        // DEBUG: Force a settlement at cell (0, 1) for testing - remove this line when done!
        if cell_x == 0 && cell_z == 1 {
            return true;
        }
        
        // Use noise at cell center for deterministic decision
        let cx = cell_x as f64 * SETTLEMENT_GRID_SIZE as f64 + SETTLEMENT_GRID_SIZE as f64 * 0.5;
        let cz = cell_z as f64 * SETTLEMENT_GRID_SIZE as f64 + SETTLEMENT_GRID_SIZE as f64 * 0.5;
        
        // Check if cell center is in desert biome
        if self.get_biome(cx as f32, cz as f32) != Biome::Desert {
            return false;
        }
        
        // Check minimum distance from spawn
        let dist_from_spawn = ((cx * cx + cz * cz) as f32).sqrt();
        if dist_from_spawn < SETTLEMENT_MIN_SPAWN_DIST {
            return false;
        }
        
        // Deterministic probability based on noise
        let noise_val = self.settlement_noise.get([cx * 0.01, cz * 0.01]) as f32;
        noise_val > 0.4 // ~15% of valid cells
    }

    /// Get the settlement center for a given grid cell (with jitter)
    fn get_cell_settlement_center(&self, cell_x: i32, cell_z: i32) -> Vec2 {
        let base_x = cell_x as f32 * SETTLEMENT_GRID_SIZE + SETTLEMENT_GRID_SIZE * 0.5;
        let base_z = cell_z as f32 * SETTLEMENT_GRID_SIZE + SETTLEMENT_GRID_SIZE * 0.5;
        
        // Add deterministic jitter so settlements aren't on a perfect grid
        let jitter_x = self.settlement_noise.get([base_x as f64 * 0.1, base_z as f64 * 0.1]) as f32
            * SETTLEMENT_GRID_SIZE * 0.2;
        let jitter_z = self.settlement_noise.get([base_z as f64 * 0.1, base_x as f64 * 0.1]) as f32
            * SETTLEMENT_GRID_SIZE * 0.2;
        
        Vec2::new(base_x + jitter_x, base_z + jitter_z)
    }

    /// Check if a world position is within a settlement zone
    /// Returns settlement info if in a settlement, None otherwise
    pub fn get_settlement_at(&self, x: f32, z: f32) -> Option<SettlementInfo> {
        // Determine which grid cell this position is in
        let cell_x = (x / SETTLEMENT_GRID_SIZE).floor() as i32;
        let cell_z = (z / SETTLEMENT_GRID_SIZE).floor() as i32;
        
        // Check this cell and adjacent cells (settlement could overlap)
        for dx in -1..=1 {
            for dz in -1..=1 {
                let cx = cell_x + dx;
                let cz = cell_z + dz;
                
                if self.cell_has_settlement(cx, cz) {
                    let center = self.get_cell_settlement_center(cx, cz);
                    let dist = ((x - center.x).powi(2) + (z - center.y).powi(2)).sqrt();
                    
                    if dist < SETTLEMENT_RADIUS {
                        // Calculate base height at center (without settlement flattening)
                        let base_height = self.get_desert_height_raw(center.x, center.y);
                        
                        return Some(SettlementInfo {
                            center,
                            radius: SETTLEMENT_RADIUS,
                            base_height,
                        });
                    }
                }
            }
        }
        
        None
    }

    /// Check if position is in a settlement zone (convenience method)
    pub fn is_in_settlement(&self, x: f32, z: f32) -> bool {
        self.get_settlement_at(x, z).is_some()
    }

    /// Get all settlement centers that could affect a chunk
    pub fn get_settlements_near_chunk(&self, chunk: ChunkCoord) -> Vec<SettlementInfo> {
        let origin = chunk.world_pos();
        let mut settlements = Vec::new();
        
        // Check a wide area around the chunk
        let check_radius = SETTLEMENT_RADIUS + CHUNK_SIZE;
        let min_cell_x = ((origin.x - check_radius) / SETTLEMENT_GRID_SIZE).floor() as i32;
        let max_cell_x = ((origin.x + CHUNK_SIZE + check_radius) / SETTLEMENT_GRID_SIZE).ceil() as i32;
        let min_cell_z = ((origin.z - check_radius) / SETTLEMENT_GRID_SIZE).floor() as i32;
        let max_cell_z = ((origin.z + CHUNK_SIZE + check_radius) / SETTLEMENT_GRID_SIZE).ceil() as i32;
        
        for cx in min_cell_x..=max_cell_x {
            for cz in min_cell_z..=max_cell_z {
                if self.cell_has_settlement(cx, cz) {
                    let center = self.get_cell_settlement_center(cx, cz);
                    let base_height = self.get_desert_height_raw(center.x, center.y);
                    
                    settlements.push(SettlementInfo {
                        center,
                        radius: SETTLEMENT_RADIUS,
                        base_height,
                    });
                }
            }
        }
        
        settlements
    }

    /// Get the biome blend value at a world position
    /// Returns a value from -1 (pure desert) to +1 (pure grasslands)
    /// Values near 0 are in the transition zone
    fn get_biome_blend(&self, x: f32, z: f32) -> f32 {
        let biome_scale = 0.001;
        let biome_value = self.biome_noise.get([
            x as f64 * biome_scale,
            z as f64 * biome_scale,
        ]) as f32;

        let dist_from_spawn = (x * x + z * z).sqrt();
        
        let spawn_bias = if dist_from_spawn < 500.0 {
            -0.8 + (dist_from_spawn / 500.0) * 0.5
        } else if dist_from_spawn < 800.0 {
            -0.3 + ((dist_from_spawn - 500.0) / 300.0) * 0.3
        } else {
            0.0
        };
        
        biome_value + spawn_bias
    }

    /// Get the biome at a world position
    pub fn get_biome(&self, x: f32, z: f32) -> Biome {
        let blend = self.get_biome_blend(x, z);
        
        // Desert is near spawn (blend < 0)
        if blend < 0.0 {
            return Biome::Desert;
        }
        
        // Use secondary noise to decide between Grasslands and Natureland
        // Natureland appears in patches within the non-desert areas
        let nature_scale = 0.0015; // Larger features than the main biome
        let nature_value = self.biome_noise_2.get([
            x as f64 * nature_scale,
            z as f64 * nature_scale,
        ]) as f32;
        
        // Natureland appears in the positive regions of the secondary noise
        // and only when far enough from spawn (blend > 0.3 for cleaner transitions)
        if nature_value > 0.2 && blend > 0.3 {
            Biome::Natureland
        } else {
            Biome::Grasslands
        }
    }

    /// Get terrain height at a world position
    /// Smoothly blends between biomes at transitions to avoid cliffs
    pub fn get_height(&self, x: f32, z: f32) -> f32 {
        let blend = self.get_biome_blend(x, z);
        let biome = self.get_biome(x, z);
        
        // Get heights from biomes
        let desert_h = self.get_desert_height(x, z);
        let grass_h = self.get_grasslands_height(x, z);
        let nature_h = self.get_natureland_height(x, z);
        
        // Smooth blend in transition zone (-0.3 to +0.3) for desert/non-desert
        let transition_width = 0.3;
        let t = ((blend / transition_width) * 0.5 + 0.5).clamp(0.0, 1.0);
        let smooth_t = t * t * (3.0 - 2.0 * t);
        
        // For non-desert areas, blend between grasslands and natureland
        let non_desert_h = match biome {
            Biome::Natureland => {
                // Blend natureland with grasslands at edges
                let nature_scale = 0.0015;
                let nature_value = self.biome_noise_2.get([
                    x as f64 * nature_scale,
                    z as f64 * nature_scale,
                ]) as f32;
                // Smooth transition at nature_value ~0.2
                let nature_t = ((nature_value - 0.1) / 0.2).clamp(0.0, 1.0);
                let nature_smooth = nature_t * nature_t * (3.0 - 2.0 * nature_t);
                grass_h * (1.0 - nature_smooth) + nature_h * nature_smooth
            }
            _ => grass_h,
        };
        
        // Blend desert with non-desert
        let base_height = desert_h * (1.0 - smooth_t) + non_desert_h * smooth_t;
        
        // Add ramp near spawn for testing jumps!
        base_height + self.get_ramp_height(x, z)
    }
    
    /// Add a jump ramp near spawn
    fn get_ramp_height(&self, x: f32, z: f32) -> f32 {
        // Ramp at position (15, 0) pointing towards spawn
        let ramp_x = 15.0;
        let ramp_z = 0.0;
        let ramp_length = 8.0;   // 8m long
        let ramp_width = 4.0;    // 4m wide
        let ramp_height = 3.0;   // 3m tall at peak
        
        // Distance from ramp center
        let dx = x - ramp_x;
        let dz = z - ramp_z;
        
        // Check if within ramp bounds (oriented along X axis, pointing towards spawn)
        let along_ramp = -dx; // Positive = towards spawn (ramp goes up towards spawn)
        let across_ramp = dz.abs();
        
        if along_ramp >= 0.0 && along_ramp <= ramp_length && across_ramp <= ramp_width / 2.0 {
            // Ramp profile: smooth rise
            let ramp_t = along_ramp / ramp_length;
            // Smooth curve that rises then flattens
            let profile = (ramp_t * std::f32::consts::PI).sin();
            // Taper width at edges
            let width_factor = 1.0 - (across_ramp / (ramp_width / 2.0)).powf(2.0);
            
            ramp_height * profile * width_factor
        } else {
            0.0
        }
    }

    /// Desert terrain: sharp dune ridges with wide valleys between them
    /// This version applies settlement flattening
    fn get_desert_height(&self, x: f32, z: f32) -> f32 {
        let raw_height = self.get_desert_height_raw(x, z);
        
        // Check if we're in a settlement zone
        if let Some(settlement) = self.get_settlement_at(x, z) {
            let dist = ((x - settlement.center.x).powi(2) + (z - settlement.center.y).powi(2)).sqrt();
            let edge_start = settlement.radius * 0.7; // Start blending at 70% of radius
            
            if dist < edge_start {
                // Fully flattened interior
                settlement.base_height
            } else {
                // Smooth blend at edges using smoothstep
                let t = (dist - edge_start) / (settlement.radius - edge_start);
                let smooth_t = t * t * (3.0 - 2.0 * t);
                settlement.base_height * (1.0 - smooth_t) + raw_height * smooth_t
            }
        } else {
            raw_height
        }
    }

    /// Raw desert height without settlement flattening
    /// Used internally to calculate base heights for settlements
    fn get_desert_height_raw(&self, x: f32, z: f32) -> f32 {
        // Very gentle base undulation (massive smooth hills)
        let base_scale = 0.002; // Features every ~500m
        let base = self.height_noise.get([
            x as f64 * base_scale,
            z as f64 * base_scale,
        ]) as f32 * 3.0; // Only 3m variation

        // Long stretched dune ridges running roughly east-west
        // Compress X axis to create elongated features
        let dune_scale_x = 0.003;  // Stretched along X
        let dune_scale_z = 0.015;  // Compressed along Z (creates ridges)
        let dunes = self.dune_noise.get([
            x as f64 * dune_scale_x,
            z as f64 * dune_scale_z,
        ]) as f32;
        
        // INVERTED: (1 - abs) puts SHARP PEAKS at zero-crossings, wide valleys away
        // Then powf < 1 makes the peaks even sharper
        let dune_shape = (1.0 - dunes.abs()).powf(1.5); // Sharp ridges!
        let dune_height = dune_shape * 10.0; // Up to 10m dune ridges

        // Secondary smaller dunes at different angle (also inverted)
        let small_dune_x = 0.008;
        let small_dune_z = 0.004;
        let small_dunes = self.dune_noise.get([
            x as f64 * small_dune_x + 500.0,
            z as f64 * small_dune_z + 500.0,
        ]) as f32;
        let small_dune_shape = (1.0 - small_dunes.abs()).powf(1.3);
        let small_dune_height = small_dune_shape * 4.0;

        // Very subtle surface ripples (wind patterns)
        let ripple_scale = 0.05;
        let ripples = self.detail_noise.get([
            x as f64 * ripple_scale,
            z as f64 * ripple_scale * 0.3, // Stretched ripples
        ]) as f32 * 0.3;

        BASE_HEIGHT + base + dune_height + small_dune_height + ripples
    }

    /// Grasslands terrain: rolling hills with more variation
    /// Scaled to have similar average height to desert for smooth transitions
    fn get_grasslands_height(&self, x: f32, z: f32) -> f32 {
        // Base elevation similar to desert (~5-8m) to avoid cliffs at biome boundaries
        let base_scale = 0.004;
        let base = self.height_noise.get([
            x as f64 * base_scale,
            z as f64 * base_scale,
        ]) as f32 * 6.0 + 5.0; // 5-11m base, similar to desert

        // Rolling hills on top
        let hill_scale = 0.012;
        let hills = self.height_noise.get([
            x as f64 * hill_scale + 100.0,
            z as f64 * hill_scale + 100.0,
        ]) as f32;
        // Smooth hills, not too tall
        let hill_height = hills.abs() * 8.0;

        // Small details/bumps
        let detail_scale = 0.04;
        let detail = self.detail_noise.get([
            x as f64 * detail_scale,
            z as f64 * detail_scale,
        ]) as f32 * 1.5;

        BASE_HEIGHT + base + hill_height + detail
    }

    /// Natureland terrain: stylized forest with varied elevation
    /// More dramatic terrain with clearings and dense forest areas
    fn get_natureland_height(&self, x: f32, z: f32) -> f32 {
        // Base elevation - slightly higher than grasslands for forest feel
        let base_scale = 0.003;
        let base = self.height_noise.get([
            x as f64 * base_scale + 200.0,
            z as f64 * base_scale + 200.0,
        ]) as f32 * 5.0 + 6.0; // 6-11m base

        // Gentle rolling terrain - forest floors
        let roll_scale = 0.008;
        let rolls = self.height_noise.get([
            x as f64 * roll_scale + 300.0,
            z as f64 * roll_scale + 300.0,
        ]) as f32;
        let roll_height = rolls * rolls * 6.0; // Squared for softer hills

        // Occasional rocky outcrops
        let rock_scale = 0.025;
        let rocks = self.dune_noise.get([
            x as f64 * rock_scale + 400.0,
            z as f64 * rock_scale + 400.0,
        ]) as f32;
        let rock_height = if rocks > 0.6 { (rocks - 0.6) * 15.0 } else { 0.0 };

        // Fine detail - roots, small bumps
        let detail_scale = 0.06;
        let detail = self.detail_noise.get([
            x as f64 * detail_scale + 500.0,
            z as f64 * detail_scale + 500.0,
        ]) as f32 * 0.8;

        BASE_HEIGHT + base + roll_height + rock_height + detail
    }

    /// Get the terrain normal at a world position
    /// This is used to align vehicles/objects with the ground slope
    pub fn get_normal(&self, x: f32, z: f32) -> Vec3 {
        // Sample heights in a small cross pattern around the point
        let sample_dist = 0.5; // Half meter sampling distance
        
        let _h_center = self.get_height(x, z);
        let h_left = self.get_height(x - sample_dist, z);
        let h_right = self.get_height(x + sample_dist, z);
        let h_back = self.get_height(x, z - sample_dist);
        let h_front = self.get_height(x, z + sample_dist);
        
        // Calculate gradient (slope) in X and Z directions
        let dx = (h_right - h_left) / (2.0 * sample_dist);
        let dz = (h_front - h_back) / (2.0 * sample_dist);
        
        // Normal is perpendicular to the surface
        // If slope is 0, normal points straight up (0, 1, 0)
        // Cross product of tangent vectors: (-dx, 1, 0) x (0, 1, -dz) = (1, dx, 0) x (0, dz, 1)
        // Simplified: normal = (-dx, 1, -dz) normalized
        Vec3::new(-dx, 1.0, -dz).normalize()
    }

    /// Generate vertex data for a chunk
    pub fn generate_chunk_vertices(&self, coord: ChunkCoord) -> ChunkMeshData {
        let origin = coord.world_pos();
        let mut positions = Vec::with_capacity(CHUNK_RESOLUTION * CHUNK_RESOLUTION);
        let mut normals = Vec::with_capacity(CHUNK_RESOLUTION * CHUNK_RESOLUTION);
        let mut uvs = Vec::with_capacity(CHUNK_RESOLUTION * CHUNK_RESOLUTION);
        let mut colors = Vec::with_capacity(CHUNK_RESOLUTION * CHUNK_RESOLUTION);
        let mut indices = Vec::new();

        // Generate vertices
        for zi in 0..CHUNK_RESOLUTION {
            for xi in 0..CHUNK_RESOLUTION {
                let local_x = xi as f32 * VERTEX_SPACING;
                let local_z = zi as f32 * VERTEX_SPACING;
                let world_x = origin.x + local_x;
                let world_z = origin.z + local_z;

                let height = self.get_height(world_x, world_z);
                let biome = self.get_biome(world_x, world_z);

                positions.push([local_x, height, local_z]);
                uvs.push([local_x / CHUNK_SIZE, local_z / CHUNK_SIZE]);
                
                // Add slight color variation based on height for visual interest
                let base_color = biome.color();
                let height_factor = (height / MAX_HEIGHT).clamp(0.0, 1.0);
                let variation = match biome {
                    Biome::Desert => {
                        // Lighter on dune crests, slightly darker in valleys
                        0.9 + height_factor * 0.15
                    }
                    Biome::Grasslands => {
                        // Greener in valleys, yellower on hills
                        0.95 + height_factor * 0.1
                    }
                    Biome::Natureland => {
                        // Darker in low areas (forest floor), lighter on outcrops
                        0.85 + height_factor * 0.2
                    }
                };
                
                let rgba = base_color.to_srgba();
                colors.push([
                    (rgba.red * variation).min(1.0),
                    (rgba.green * variation).min(1.0),
                    (rgba.blue * variation).min(1.0),
                    1.0,
                ]);
            }
        }

        // Calculate normals
        for zi in 0..CHUNK_RESOLUTION {
            for xi in 0..CHUNK_RESOLUTION {
                let idx = zi * CHUNK_RESOLUTION + xi;
                
                let h_left = if xi > 0 { positions[idx - 1][1] } else { positions[idx][1] };
                let h_right = if xi < CHUNK_RESOLUTION - 1 { positions[idx + 1][1] } else { positions[idx][1] };
                let h_down = if zi > 0 { positions[idx - CHUNK_RESOLUTION][1] } else { positions[idx][1] };
                let h_up = if zi < CHUNK_RESOLUTION - 1 { positions[idx + CHUNK_RESOLUTION][1] } else { positions[idx][1] };

                let normal = Vec3::new(
                    h_left - h_right,
                    2.0 * VERTEX_SPACING,
                    h_down - h_up,
                ).normalize();

                normals.push([normal.x, normal.y, normal.z]);
            }
        }

        // Generate triangle indices
        for zi in 0..(CHUNK_RESOLUTION - 1) {
            for xi in 0..(CHUNK_RESOLUTION - 1) {
                let top_left = (zi * CHUNK_RESOLUTION + xi) as u32;
                let top_right = top_left + 1;
                let bottom_left = top_left + CHUNK_RESOLUTION as u32;
                let bottom_right = bottom_left + 1;

                indices.push(top_left);
                indices.push(bottom_left);
                indices.push(top_right);

                indices.push(top_right);
                indices.push(bottom_left);
                indices.push(bottom_right);
            }
        }

        // Dominant biome for chunk
        let center_x = origin.x + CHUNK_SIZE / 2.0;
        let center_z = origin.z + CHUNK_SIZE / 2.0;
        let biome = self.get_biome(center_x, center_z);

        ChunkMeshData {
            positions,
            normals,
            uvs,
            colors,
            indices,
            biome,
        }
    }
}

/// Generated mesh data for a terrain chunk
pub struct ChunkMeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    pub biome: Biome,
}

/// Resource holding the terrain generator
#[derive(Resource)]
pub struct WorldTerrain {
    pub generator: TerrainGenerator,
}

impl Default for WorldTerrain {
    fn default() -> Self {
        Self {
            generator: TerrainGenerator::new(WORLD_SEED),
        }
    }
}

/// View distance in chunks (64m chunks, 6 chunks = 384m view distance)
pub const VIEW_DISTANCE: i32 = 6;
