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
}

impl Biome {
    /// Get the color for this biome
    pub fn color(&self) -> Color {
        match self {
            // Warm golden sand
            Biome::Desert => Color::srgb(0.85, 0.75, 0.55),
            // Lush green grass
            Biome::Grasslands => Color::srgb(0.35, 0.55, 0.25),
        }
    }

    /// Get a secondary accent color for variation
    pub fn accent_color(&self) -> Color {
        match self {
            Biome::Desert => Color::srgb(0.90, 0.80, 0.60),
            Biome::Grasslands => Color::srgb(0.40, 0.60, 0.30),
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

/// Terrain generator using Perlin noise
pub struct TerrainGenerator {
    height_noise: Perlin,
    biome_noise: Perlin,
    dune_noise: Perlin,
    detail_noise: Perlin,
    #[allow(dead_code)]
    seed: u32,
}

impl TerrainGenerator {
    pub fn new(seed: u32) -> Self {
        Self {
            height_noise: Perlin::new(seed),
            biome_noise: Perlin::new(seed.wrapping_add(1000)),
            dune_noise: Perlin::new(seed.wrapping_add(2000)),
            detail_noise: Perlin::new(seed.wrapping_add(3000)),
            seed,
        }
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
        if self.get_biome_blend(x, z) < 0.0 {
            Biome::Desert
        } else {
            Biome::Grasslands
        }
    }

    /// Get terrain height at a world position
    /// Smoothly blends between biomes at transitions to avoid cliffs
    pub fn get_height(&self, x: f32, z: f32) -> f32 {
        let blend = self.get_biome_blend(x, z);
        
        // Get heights from both biomes
        let desert_h = self.get_desert_height(x, z);
        let grass_h = self.get_grasslands_height(x, z);
        
        // Smooth blend in transition zone (-0.3 to +0.3)
        // Outside this range, use pure biome height
        let transition_width = 0.3;
        let t = ((blend / transition_width) * 0.5 + 0.5).clamp(0.0, 1.0);
        
        // Smoothstep for nicer transition
        let smooth_t = t * t * (3.0 - 2.0 * t);
        
        // Blend heights
        let base_height = desert_h * (1.0 - smooth_t) + grass_h * smooth_t;
        
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
    fn get_desert_height(&self, x: f32, z: f32) -> f32 {
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
