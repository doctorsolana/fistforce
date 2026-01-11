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
        app.add_systems(
            Update,
            (update_terrain_chunks, spawn_terrain_chunks)
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}

/// Determine which chunks should be loaded based on player position
fn update_terrain_chunks(
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    chunk_query: Query<(Entity, &TerrainChunk)>,
    mut commands: Commands,
) {
    let Ok(player_pos) = player_query.single() else {
        return;
    };

    let player_chunk = ChunkCoord::from_world_pos(player_pos.0);
    let desired_chunks: HashSet<ChunkCoord> = player_chunk
        .chunks_in_radius(VIEW_DISTANCE)
        .into_iter()
        .collect();

    // Unload chunks that are too far
    let chunks_to_unload: Vec<ChunkCoord> = loaded_chunks
        .chunks
        .difference(&desired_chunks)
        .cloned()
        .collect();

    for coord in chunks_to_unload {
        // Find and despawn the chunk entity
        for (entity, chunk) in chunk_query.iter() {
            if chunk.coord == coord {
                commands.entity(entity).despawn();
                loaded_chunks.chunks.remove(&coord);
                break;
            }
        }
    }
}

/// Spawn terrain chunks that should be loaded but aren't yet
fn spawn_terrain_chunks(
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    terrain: Res<WorldTerrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
    mut commands: Commands,
) {
    let Ok(player_pos) = player_query.single() else {
        return;
    };

    let Ok(world_root) = world_root_query.single() else {
        return;
    };

    let player_chunk = ChunkCoord::from_world_pos(player_pos.0);
    let desired_chunks: HashSet<ChunkCoord> = player_chunk
        .chunks_in_radius(VIEW_DISTANCE)
        .into_iter()
        .collect();

    // Load new chunks (limit per frame to avoid stutter)
    let mut chunks_spawned = 0;
    let max_chunks_per_frame = 2;

    for coord in desired_chunks.iter() {
        if chunks_spawned >= max_chunks_per_frame {
            break;
        }

        if !loaded_chunks.chunks.contains(coord) {
            // Generate chunk mesh
            let mesh_data = terrain.generator.generate_chunk_vertices(*coord);
            let chunk_pos = coord.world_pos();

            // Create mesh
            let mut mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
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

            // Create material based on dominant biome
            let material = materials.add(StandardMaterial {
                base_color: mesh_data.biome.color(),
                perceptual_roughness: 0.9,
                metallic: 0.0,
                // Use vertex colors for variation
                ..default()
            });

            // Spawn chunk entity
            let chunk_entity = commands
                .spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(material),
                    Transform::from_translation(chunk_pos),
                    TerrainChunk { coord: *coord },
                ))
                .id();

            commands.entity(world_root).add_child(chunk_entity);
            loaded_chunks.chunks.insert(*coord);
            chunks_spawned += 1;
        }
    }
}
