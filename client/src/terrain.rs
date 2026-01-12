//! Client-side terrain rendering
//!
//! Updated for Bevy 0.17

use bevy::prelude::*;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::render::render_resource::{PrimitiveTopology, Extent3d, TextureDimension, TextureFormat};
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageSampler, ImageAddressMode, ImageSamplerDescriptor, ImageFilterMode};
use noise::{NoiseFn, Perlin, Fbm, MultiFractal};
use std::collections::HashSet;

use shared::{
    ChunkCoord, LocalPlayer, PlayerPosition, WorldTerrain, VIEW_DISTANCE, WORLD_SEED,
};

use crate::states::GameState;
use crate::systems::ClientWorldRoot;

/// Marker component for terrain chunk entities
#[derive(Component)]
pub struct TerrainChunk {
    pub coord: ChunkCoord,
}

/// Tracks which chunk the player is currently in and the desired chunk ordering.
#[derive(Resource, Default)]
pub struct TerrainStreamingState {
    pub center: Option<ChunkCoord>,
    /// Desired chunks sorted from nearest -> farthest (spawn near chunks first).
    pub desired_order: Vec<ChunkCoord>,
}

/// Shared terrain render assets (avoid per-chunk material allocations).
#[derive(Resource)]
pub struct TerrainRenderAssets {
    pub material: Handle<StandardMaterial>,
    // Texture handles kept alive to prevent GPU resource cleanup
    #[allow(dead_code)]
    detail_texture: Handle<Image>,
    #[allow(dead_code)]
    normal_map: Handle<Image>,
}

/// Resource tracking which chunks are currently loaded
#[derive(Resource, Default)]
pub struct LoadedChunks {
    pub chunks: HashSet<ChunkCoord>,
}

/// Plugin for terrain rendering
pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadedChunks>();
        app.init_resource::<WorldTerrain>();
        app.init_resource::<TerrainStreamingState>();
        app.add_systems(Startup, setup_terrain_render_assets);
        app.add_systems(
            Update,
            (update_terrain_chunks, spawn_terrain_chunks)
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}

/// Generate a procedural noise texture for terrain detail
fn generate_detail_texture(size: u32) -> Image {
    let fbm: Fbm<Perlin> = Fbm::new(WORLD_SEED)
        .set_octaves(3)
        .set_frequency(1.0)
        .set_persistence(0.4);
    
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    
    for y in 0..size {
        for x in 0..size {
            // Normalize to 0-1 range for seamless tiling
            let nx = x as f64 / size as f64;
            let ny = y as f64 / size as f64;
            
            // Single noise layer with subtle variation
            let scale = 6.0;
            let noise = fbm.get([nx * scale, ny * scale]) as f32;
            
            // Very subtle variation: 0.94 - 1.0 range (only 6% darkening max)
            // This keeps sand bright while adding just enough texture to break uniformity
            let value = ((noise * 0.5 + 0.5) * 0.06 + 0.94).clamp(0.94, 1.0);
            let byte = (value * 255.0) as u8;
            
            data.push(byte); // R
            data.push(byte); // G
            data.push(byte); // B
            data.push(255);  // A
        }
    }
    
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    
    // Enable seamless tiling
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    
    image
}

/// Generate a procedural normal map for surface bumps
fn generate_normal_map(size: u32) -> Image {
    let fbm: Fbm<Perlin> = Fbm::new(WORLD_SEED.wrapping_add(1000))
        .set_octaves(3)
        .set_frequency(1.0)
        .set_persistence(0.5);
    
    // First pass: generate height values
    let mut heights = vec![0.0f32; (size * size) as usize];
    for y in 0..size {
        for x in 0..size {
            let nx = x as f64 / size as f64;
            let ny = y as f64 / size as f64;
            
            let scale = 5.0; // Larger scale = softer bumps
            let height = fbm.get([nx * scale, ny * scale]) as f32;
            heights[(y * size + x) as usize] = height;
        }
    }
    
    // Second pass: compute normals from height differences
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    let strength = 0.6; // Subtle bumps - sand is relatively smooth
    
    for y in 0..size {
        for x in 0..size {
            // Sample neighbors (with wrapping for seamless tiling)
            let left = heights[(y * size + (x + size - 1) % size) as usize];
            let right = heights[(y * size + (x + 1) % size) as usize];
            let up = heights[((y + size - 1) % size * size + x) as usize];
            let down = heights[((y + 1) % size * size + x) as usize];
            
            // Compute normal from height gradient
            let dx = (right - left) * strength;
            let dy = (down - up) * strength;
            
            // Normal in tangent space (Z-up convention for normal maps)
            let normal = Vec3::new(-dx, -dy, 1.0).normalize();
            
            // Map from [-1,1] to [0,255]
            data.push(((normal.x * 0.5 + 0.5) * 255.0) as u8); // R
            data.push(((normal.y * 0.5 + 0.5) * 255.0) as u8); // G
            data.push(((normal.z * 0.5 + 0.5) * 255.0) as u8); // B
            data.push(255); // A
        }
    }
    
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm, // Normal maps use linear color space
        RenderAssetUsages::RENDER_WORLD,
    );
    
    // Enable seamless tiling
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    
    image
}

/// Create shared terrain material once.
fn setup_terrain_render_assets(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Generate procedural textures
    let detail_texture = images.add(generate_detail_texture(256));
    let normal_map = images.add(generate_normal_map(256));
    
    // Terrain material with procedural textures
    // Vertex colors provide biome tint; detail texture adds surface variation
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(detail_texture.clone()),
        normal_map_texture: Some(normal_map.clone()),
        perceptual_roughness: 0.85, // Higher roughness = less shiny/clay-like
        metallic: 0.0,
        reflectance: 0.2,
        // UV tiling: repeat texture every 8 meters for good detail density
        uv_transform: bevy::math::Affine2::from_scale(Vec2::splat(8.0)),
        ..default()
    });

    commands.insert_resource(TerrainRenderAssets { 
        material,
        detail_texture,
        normal_map,
    });
    
    info!("Generated procedural terrain textures (256x256 detail + normal map)");
}

/// Determine which chunks should be loaded based on player position
fn update_terrain_chunks(
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    mut streaming: ResMut<TerrainStreamingState>,
    chunk_query: Query<(Entity, &TerrainChunk)>,
    mut commands: Commands,
) {
    let Ok(player_pos) = player_query.single() else {
        return;
    };

    let player_chunk = ChunkCoord::from_world_pos(player_pos.0);

    // Only recompute desired chunks when we cross into a new chunk.
    if streaming.center == Some(player_chunk) {
        return;
    }
    streaming.center = Some(player_chunk);

    // Update desired chunk ordering (nearest -> farthest).
    let mut desired: Vec<ChunkCoord> = player_chunk.chunks_in_radius(VIEW_DISTANCE);
    desired.sort_by_key(|c| {
        let dx = (c.x - player_chunk.x).abs();
        let dz = (c.z - player_chunk.z).abs();
        // Chebyshev distance for square radius ordering
        dx.max(dz)
    });
    streaming.desired_order = desired;

    // Unload chunks that are now out of range. Do this by scanning existing chunk entities
    // (avoids HashSet difference + nested loops).
    let mut to_remove: Vec<(Entity, ChunkCoord)> = Vec::new();
    for (entity, chunk) in chunk_query.iter() {
        let dx = (chunk.coord.x - player_chunk.x).abs();
        let dz = (chunk.coord.z - player_chunk.z).abs();
        if dx > VIEW_DISTANCE || dz > VIEW_DISTANCE {
            to_remove.push((entity, chunk.coord));
        }
    }
    for (entity, coord) in to_remove {
        commands.entity(entity).despawn();
        loaded_chunks.chunks.remove(&coord);
    }
}

/// Spawn terrain chunks that should be loaded but aren't yet
fn spawn_terrain_chunks(
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    streaming: Res<TerrainStreamingState>,
    terrain: Res<WorldTerrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    render_assets: Option<Res<TerrainRenderAssets>>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
    mut commands: Commands,
) {
    let Ok(player_pos) = player_query.single() else {
        return;
    };

    let Ok(world_root) = world_root_query.single() else {
        return;
    };

    let Some(render_assets) = render_assets else { return };

    // Load new chunks (limit per frame to avoid stutter)
    let mut chunks_spawned = 0;
    let max_chunks_per_frame = 2;

    // If streaming hasn't initialized yet (no movement / no update), fall back to computing once.
    let desired_iter: Box<dyn Iterator<Item = ChunkCoord>> = if let Some(center) = streaming.center {
        if center == ChunkCoord::from_world_pos(player_pos.0) && !streaming.desired_order.is_empty()
        {
            Box::new(streaming.desired_order.iter().copied())
        } else {
            // Center mismatch (should be rare): compute locally.
            Box::new(
                ChunkCoord::from_world_pos(player_pos.0)
                    .chunks_in_radius(VIEW_DISTANCE)
                    .into_iter(),
            )
        }
    } else {
        Box::new(
            ChunkCoord::from_world_pos(player_pos.0)
                .chunks_in_radius(VIEW_DISTANCE)
                .into_iter(),
        )
    };

    for coord in desired_iter {
        if chunks_spawned >= max_chunks_per_frame {
            break;
        }

        if !loaded_chunks.chunks.contains(&coord) {
            // Generate chunk mesh
            let mesh_data = terrain.generator.generate_chunk_vertices(coord);
            let chunk_pos = coord.world_pos();

            // Create mesh
            let mut mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                // Performance: once uploaded, we don't need CPU access to terrain meshes.
                RenderAssetUsages::RENDER_WORLD,
            );

            mesh.insert_attribute(
                Mesh::ATTRIBUTE_POSITION,
                VertexAttributeValues::Float32x3(mesh_data.positions),
            );
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_NORMAL,
                VertexAttributeValues::Float32x3(mesh_data.normals),
            );
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_UV_0,
                VertexAttributeValues::Float32x2(mesh_data.uvs),
            );
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_COLOR,
                VertexAttributeValues::Float32x4(mesh_data.colors),
            );
            mesh.insert_indices(Indices::U32(mesh_data.indices));

            // Spawn chunk entity
            let chunk_entity = commands
                .spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(render_assets.material.clone()),
                    Transform::from_translation(chunk_pos),
                    TerrainChunk { coord },
                ))
                .id();

            commands.entity(world_root).add_child(chunk_entity);
            loaded_chunks.chunks.insert(coord);
            chunks_spawned += 1;
        }
    }
}
