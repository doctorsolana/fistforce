//! Environmental props - rocks, trees, grass, etc.
//!
//! Spawns decorative assets based on biome type using deterministic placement.

use bevy::prelude::*;
use noise::{NoiseFn, Perlin};
use shared::{terrain::{Biome, ChunkCoord, CHUNK_SIZE}, WorldTerrain};

use crate::terrain::LoadedChunks;
use crate::systems::ClientWorldRoot;
use crate::states::GameState;

/// Marker for environment prop entities
#[derive(Component)]
pub struct EnvironmentProp {
    pub chunk: ChunkCoord,
}

/// Tracks which chunks have had props spawned
#[derive(Resource, Default)]
pub struct LoadedPropChunks {
    pub chunks: std::collections::HashSet<ChunkCoord>,
}

/// Handles to loaded prop assets
#[derive(Resource)]
pub struct PropAssets {
    // Desert props
    pub rocks: Vec<Handle<Scene>>,
    pub dead_trees: Vec<Handle<Scene>>,
    // Grassland props  
    pub trees: Vec<Handle<Scene>>,
    pub bushes: Vec<Handle<Scene>>,
    pub grass: Vec<Handle<Scene>>,
}

/// Plugin for environmental props
pub struct PropsPlugin;

impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadedPropChunks>();
        app.add_systems(Startup, load_prop_assets);
        app.add_systems(
            Update,
            (spawn_chunk_props, cleanup_chunk_props)
                .run_if(in_state(GameState::Playing)),
        );
    }
}

/// Load all prop GLTF assets at startup
fn load_prop_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    let rocks = vec![
        asset_server.load("Assetsfromassetpack/gltf/Rock_1_A_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Rock_1_B_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Rock_1_C_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Rock_2_A_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Rock_2_B_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Rock_3_A_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Rock_3_B_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Rock_3_C_Color1.gltf#Scene0"),
    ];

    let dead_trees = vec![
        asset_server.load("Assetsfromassetpack/gltf/Tree_Bare_1_A_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Tree_Bare_1_B_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Tree_Bare_2_A_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Tree_Bare_2_B_Color1.gltf#Scene0"),
    ];

    let trees = vec![
        asset_server.load("Assetsfromassetpack/gltf/Tree_1_A_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Tree_1_B_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Tree_2_A_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Tree_2_B_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Tree_3_A_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Tree_4_A_Color1.gltf#Scene0"),
    ];

    let bushes = vec![
        asset_server.load("Assetsfromassetpack/gltf/Bush_1_A_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Bush_1_B_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Bush_2_A_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Bush_2_B_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Bush_3_A_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Bush_4_A_Color1.gltf#Scene0"),
    ];

    let grass = vec![
        asset_server.load("Assetsfromassetpack/gltf/Grass_1_A_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Grass_1_B_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Grass_2_A_Color1.gltf#Scene0"),
        asset_server.load("Assetsfromassetpack/gltf/Grass_2_B_Color1.gltf#Scene0"),
    ];

    commands.insert_resource(PropAssets {
        rocks,
        dead_trees,
        trees,
        bushes,
        grass,
    });

    info!("Loaded environmental prop assets");
}

/// Spawn props for newly loaded terrain chunks
fn spawn_chunk_props(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    prop_assets: Option<Res<PropAssets>>,
    loaded_chunks: Res<LoadedChunks>,
    mut loaded_prop_chunks: ResMut<LoadedPropChunks>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
) {
    let Some(assets) = prop_assets else { return };
    let Ok(world_root) = world_root_query.single() else { return };

    // Use deterministic noise for prop placement
    let placement_noise = Perlin::new(shared::terrain::WORLD_SEED.wrapping_add(5000));
    let density_noise = Perlin::new(shared::terrain::WORLD_SEED.wrapping_add(6000));
    let variety_noise = Perlin::new(shared::terrain::WORLD_SEED.wrapping_add(7000));

    // Find chunks that need props
    for coord in loaded_chunks.chunks.iter() {
        if loaded_prop_chunks.chunks.contains(coord) {
            continue;
        }

        let chunk_origin = coord.world_pos();
        
        // Sample points within the chunk for prop placement
        // Use a grid with noise-based jitter for natural distribution
        let grid_spacing = 8.0; // Base spacing between potential prop positions
        let steps = (CHUNK_SIZE / grid_spacing) as i32;

        for gz in 0..steps {
            for gx in 0..steps {
                let base_x = chunk_origin.x + gx as f32 * grid_spacing + grid_spacing * 0.5;
                let base_z = chunk_origin.z + gz as f32 * grid_spacing + grid_spacing * 0.5;

                // Add deterministic jitter
                let jitter_x = placement_noise.get([base_x as f64 * 0.1, base_z as f64 * 0.1]) as f32 * grid_spacing * 0.4;
                let jitter_z = placement_noise.get([base_z as f64 * 0.1, base_x as f64 * 0.1]) as f32 * grid_spacing * 0.4;
                
                let world_x = base_x + jitter_x;
                let world_z = base_z + jitter_z;
                let world_y = terrain.generator.get_height(world_x, world_z);
                
                // Get biome and density at this position
                let biome = terrain.generator.get_biome(world_x, world_z);
                let density = density_noise.get([world_x as f64 * 0.05, world_z as f64 * 0.05]) as f32;
                let variety = variety_noise.get([world_x as f64 * 0.3, world_z as f64 * 0.3]) as f32;

                // Spawn props based on biome
                match biome {
                    Biome::Desert => {
                        spawn_desert_prop(
                            &mut commands,
                            &assets,
                            world_root,
                            *coord,
                            Vec3::new(world_x, world_y, world_z),
                            density,
                            variety,
                        );
                    }
                    Biome::Grasslands => {
                        spawn_grassland_prop(
                            &mut commands,
                            &assets,
                            world_root,
                            *coord,
                            Vec3::new(world_x, world_y, world_z),
                            density,
                            variety,
                        );
                    }
                }
            }
        }

        loaded_prop_chunks.chunks.insert(*coord);
    }
}

/// Spawn desert props (rocks + rare dead trees)
fn spawn_desert_prop(
    commands: &mut Commands,
    assets: &PropAssets,
    world_root: Entity,
    chunk: ChunkCoord,
    position: Vec3,
    density: f32,
    variety: f32,
) {
    // Rocks: ~30% chance at each grid point
    if density > 0.2 {
        let rock_idx = ((variety.abs() * 100.0) as usize) % assets.rocks.len();
        let rotation = Quat::from_rotation_y(variety * std::f32::consts::TAU);
        let scale = 0.8 + variety.abs() * 0.6; // Random scale 0.8-1.4
        
        let prop = commands.spawn((
            EnvironmentProp { chunk },
            SceneRoot(assets.rocks[rock_idx].clone()),
            Transform::from_translation(position)
                .with_rotation(rotation)
                .with_scale(Vec3::splat(scale)),
        )).id();
        commands.entity(world_root).add_child(prop);
    }
    // Dead trees: ~5% chance (very rare)
    else if density > 0.1 && density < 0.15 {
        let tree_idx = ((variety.abs() * 100.0) as usize) % assets.dead_trees.len();
        let rotation = Quat::from_rotation_y(variety * std::f32::consts::TAU);
        let scale = 1.5 + variety.abs() * 0.5; // Larger scale for trees
        
        let prop = commands.spawn((
            EnvironmentProp { chunk },
            SceneRoot(assets.dead_trees[tree_idx].clone()),
            Transform::from_translation(position)
                .with_rotation(rotation)
                .with_scale(Vec3::splat(scale)),
        )).id();
        commands.entity(world_root).add_child(prop);
    }
}

/// Spawn grassland props (trees, bushes, grass)
fn spawn_grassland_prop(
    commands: &mut Commands,
    assets: &PropAssets,
    world_root: Entity,
    chunk: ChunkCoord,
    position: Vec3,
    density: f32,
    variety: f32,
) {
    // Trees: ~15% chance
    if density > 0.35 {
        let tree_idx = ((variety.abs() * 100.0) as usize) % assets.trees.len();
        let rotation = Quat::from_rotation_y(variety * std::f32::consts::TAU);
        let scale = 1.2 + variety.abs() * 0.8; // Scale 1.2-2.0
        
        let prop = commands.spawn((
            EnvironmentProp { chunk },
            SceneRoot(assets.trees[tree_idx].clone()),
            Transform::from_translation(position)
                .with_rotation(rotation)
                .with_scale(Vec3::splat(scale)),
        )).id();
        commands.entity(world_root).add_child(prop);
    }
    // Bushes: ~20% chance
    else if density > 0.15 && density < 0.35 {
        let bush_idx = ((variety.abs() * 100.0) as usize) % assets.bushes.len();
        let rotation = Quat::from_rotation_y(variety * std::f32::consts::TAU);
        let scale = 0.8 + variety.abs() * 0.4;
        
        let prop = commands.spawn((
            EnvironmentProp { chunk },
            SceneRoot(assets.bushes[bush_idx].clone()),
            Transform::from_translation(position)
                .with_rotation(rotation)
                .with_scale(Vec3::splat(scale)),
        )).id();
        commands.entity(world_root).add_child(prop);
    }
    // Grass: ~25% chance  
    else if density > -0.1 && density < 0.15 {
        let grass_idx = ((variety.abs() * 100.0) as usize) % assets.grass.len();
        let rotation = Quat::from_rotation_y(variety * std::f32::consts::TAU);
        let scale = 0.6 + variety.abs() * 0.3;
        
        let prop = commands.spawn((
            EnvironmentProp { chunk },
            SceneRoot(assets.grass[grass_idx].clone()),
            Transform::from_translation(position)
                .with_rotation(rotation)
                .with_scale(Vec3::splat(scale)),
        )).id();
        commands.entity(world_root).add_child(prop);
    }
}

/// Clean up props when their chunk is unloaded
fn cleanup_chunk_props(
    mut commands: Commands,
    loaded_chunks: Res<LoadedChunks>,
    mut loaded_prop_chunks: ResMut<LoadedPropChunks>,
    props: Query<(Entity, &EnvironmentProp)>,
) {
    // Find chunks that are no longer loaded
    let chunks_to_remove: Vec<ChunkCoord> = loaded_prop_chunks
        .chunks
        .difference(&loaded_chunks.chunks)
        .cloned()
        .collect();

    for coord in chunks_to_remove {
        // Despawn all props in this chunk
        for (entity, prop) in props.iter() {
            if prop.chunk == coord {
                commands.entity(entity).despawn();
            }
        }
        loaded_prop_chunks.chunks.remove(&coord);
    }
}
