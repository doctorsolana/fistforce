//! Client-side terrain rendering
//!
//! Updated for Bevy 0.17

use bevy::prelude::*;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::render::render_resource::PrimitiveTopology;
use bevy::asset::RenderAssetUsages;
use std::collections::HashSet;

use shared::{
    ChunkCoord, LocalPlayer, PlayerPosition, WorldTerrain,  VIEW_DISTANCE,
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

/// Create shared terrain material once.
fn setup_terrain_render_assets(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Vertex colors provide biome tint/variation; keep base color white to preserve them.
    // Lower roughness keeps terrain colors vivid under sunlight.
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.65,
        metallic: 0.0,
        reflectance: 0.3, // subtle specular highlight on wet/sandy surfaces
        ..default()
    });

    commands.insert_resource(TerrainRenderAssets { material });
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
